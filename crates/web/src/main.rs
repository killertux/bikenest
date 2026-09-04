use bikenest_infrastructure::{Config, Db, S3ObjectStorage};
use bikenest_web::{RouterDeps, app_router_with};
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    // Load .env from the workspace root if present (dev convenience; §10).
    dotenvy::dotenv().ok();

    // The single configuration read of the whole process: every knob below,
    // and everything the router and the worker use, comes from this value.
    let config = Config::from_env().unwrap_or_else(|err| {
        eprintln!("configuration error: {err}");
        eprintln!("hint: copy .env.example to .env and adjust values");
        std::process::exit(1);
    });

    // Logging (§86): JSON structured in production (machine-parseable, forwarded to
    // a log driver/aggregator), human-readable in dev. `RUST_LOG` overrides the level.
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    if config.app_env.is_production() {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .json()
            .with_current_span(true)
            .with_span_list(true)
            .init();
    } else {
        tracing_subscriber::fmt().with_env_filter(filter).init();
    }

    // The subcommand is read up front so production validation can run before
    // any connection attempt: a misconfigured deploy sees the whole list of
    // missing settings first, not a database error.
    let subcommand = std::env::args().nth(1);
    if matches!(subcommand.as_deref(), None | Some("serve"))
        && let Err(problems) = config.validate_for_production()
    {
        eprintln!("refusing to start: APP_ENV=production requires the following settings");
        for problem in &problems {
            eprintln!("  - {problem}");
        }
        eprintln!("see docs/deployment.md (startup validation) and .env.example");
        std::process::exit(1);
    }

    let db = Db::connect(&config.database_url)
        .await
        .unwrap_or_else(|err| {
            eprintln!("database connection error: {err}");
            std::process::exit(1);
        });

    // Subcommand dispatch: default = serve the app.
    match subcommand.as_deref() {
        Some("seed-mock") => {
            // Explicit, reproducible migrations first (§10).
            db.migrate().await.unwrap_or_else(|err| {
                eprintln!("migration error: {err}");
                std::process::exit(1);
            });
            let storage = S3ObjectStorage::from_config(&config.storage);
            let processor = bikenest_infrastructure::LocalImageProcessor::new(config.photo);
            match bikenest_infrastructure::parking::seed_mock(&db, &storage, &processor).await {
                Ok(n) => {
                    println!(
                        "seeded {n} mock parking locations + photos + reviews (dev only); every photo verified retrievable"
                    );
                }
                Err(err) => {
                    eprintln!("seed error: {err}");
                    std::process::exit(1);
                }
            }
        }
        // Ledger #10: idempotent, env-driven admin bootstrap (never HTTP).
        Some("seed-admin") => {
            db.migrate().await.unwrap_or_else(|err| {
                eprintln!("migration error: {err}");
                std::process::exit(1);
            });
            match bikenest_infrastructure::seed_admin(&db).await {
                Ok(bikenest_infrastructure::auth::seed::SeedOutcome::Created) => {
                    println!("admin account created (Ledger #10)");
                }
                Ok(bikenest_infrastructure::auth::seed::SeedOutcome::Updated) => {
                    println!("admin account updated (Ledger #10)");
                }
                Err(err) => {
                    eprintln!("seed-admin error: {err}");
                    eprintln!("hint: set ADMIN_EMAIL and ADMIN_PASSWORD in .env and retry");
                    std::process::exit(1);
                }
            }
        }
        Some("retention") => {
            db.migrate().await.unwrap_or_else(|err| {
                eprintln!("migration error: {err}");
                std::process::exit(1);
            });
            let storage = S3ObjectStorage::from_config(&config.storage);
            let retention = bikenest_infrastructure::SqlxRetentionRepository::new(
                db.clone(),
                config.retention,
                Arc::new(storage),
                config.media_root.clone(),
            );
            let job = bikenest_application::RetentionJob::new(
                Box::new(retention),
                Box::new(bikenest_infrastructure::SqlxAuditLog::new(db.clone())),
                Box::new(bikenest_infrastructure::SystemClock),
                bikenest_application::RetentionConfig {
                    inactive_account_anonymize_after_days: config
                        .inactive_account_anonymize_after_days,
                    deleted_account_purge_after_days: config.deleted_account_purge_after_days,
                },
            );
            match job.run().await {
                Ok(summary) => {
                    println!("retention run ({})", summary.steps.len());
                    for step in &summary.steps {
                        println!("  {:<28} {}", step.name, step.purged);
                    }
                    println!("audited: retention.purged");
                }
                Err(err) => {
                    eprintln!("retention error: {err}");
                    std::process::exit(1);
                }
            }
        }
        Some("seed-policies") => {
            db.migrate().await.unwrap_or_else(|err| {
                eprintln!("migration error: {err}");
                std::process::exit(1);
            });
            let version = config.policy.version.clone();
            let effective_at = config.policy.effective_at;
            // §70: controller identity + contact come from the environment
            // (POLICY_OPERATOR_*, POLICY_CONTACT_EMAIL) and are substituted
            // into the {{TOKEN}}s of policies/*.md. Never seed with a hole.
            let lookup = |token: &str| -> Option<String> { config.policy.placeholder(token) };
            let mut ok = true;
            for locale in bikenest_infrastructure::POLICY_LOCALES {
                for kind_code in ["privacy", "terms", "cookies"] {
                    let file = format!("policies/{kind_code}.{locale}.md");
                    let raw = match std::fs::read_to_string(&file) {
                        Ok(c) => c,
                        Err(err) => {
                            eprintln!("seed-policies: cannot read {file}: {err}");
                            ok = false;
                            continue;
                        }
                    };
                    let content = match bikenest_infrastructure::fill_policy_placeholders(
                        &raw, lookup,
                    ) {
                        Ok(c) => c,
                        Err(missing) => {
                            let vars: Vec<&str> = bikenest_infrastructure::POLICY_PLACEHOLDERS
                                .iter()
                                .filter(|(t, _)| missing.iter().any(|m| m == t))
                                .map(|(_, var)| *var)
                                .collect();
                            eprintln!(
                                "seed-policies: {file}: unresolved placeholders {missing:?} — set {} (see .env.example)",
                                vars.join(", ")
                            );
                            ok = false;
                            continue;
                        }
                    };
                    let kind = bikenest_domain::PolicyKind::from_code(kind_code)
                        .expect("valid policy kind");
                    if let Err(err) = bikenest_infrastructure::seed_policy(
                        &db,
                        kind,
                        locale,
                        &version,
                        effective_at,
                        &content,
                    )
                    .await
                    {
                        eprintln!("seed-policies: {kind_code} ({locale}): {err}");
                        ok = false;
                    }
                }
            }
            if ok {
                println!(
                    "seeded policies version {version} for locales {:?} (legal text drafted by product; counsel review tracked in docs/legal-review.md)",
                    bikenest_infrastructure::POLICY_LOCALES
                );
            } else {
                std::process::exit(1);
            }
        }
        _ => serve(config, db).await,
    }
}

async fn serve(config: Config, db: Db) {
    // Production validation already ran in `main` (before the database
    // connection). Development runs on fakes by design; say which ones, once.
    for fake in config.fakes_in_use() {
        tracing::warn!(component = fake, "development fake in use");
    }

    let config = Arc::new(config);

    // Explicit, reproducible migrations on startup (dev workflow; §10).
    if let Err(err) = db.migrate().await {
        eprintln!("migration error: {err}");
        std::process::exit(1);
    }
    tracing::info!(migrations = "applied", "database ready");

    let deps = RouterDeps::from_config(&config).unwrap_or_else(|err| {
        eprintln!("provider configuration error: {err}");
        std::process::exit(1);
    });

    // Background worker (plans/m9-background-jobs.md): a tokio task that claims
    // and runs durable one-shot + recurring jobs. Disable with `JOBS_ENABLED=false`
    // for web-only instances (or to run jobs elsewhere).
    if config.jobs.enabled {
        let storage: Arc<dyn bikenest_application::ObjectStorage> =
            Arc::new(S3ObjectStorage::from_config(&config.storage));
        let services = bikenest_infrastructure::job_services(db.clone(), &config, storage);
        let worker =
            bikenest_infrastructure::Worker::new(services.repo, services.registry, config.jobs);
        tokio::spawn(worker.run());
        tracing::info!(jobs = "enabled", "background worker started");
    } else {
        tracing::info!(jobs = "disabled", "background worker not started");
    }

    let bind_addr = config.bind_addr.clone();
    let app = app_router_with(config, db, deps);
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .unwrap_or_else(|err| {
            eprintln!("failed to bind {bind_addr}: {err}");
            std::process::exit(1);
        });
    tracing::info!(addr = %bind_addr, "bikenest listening");
    axum::serve(listener, app).await.expect("server error");
}
