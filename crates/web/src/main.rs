use bikesnest_infrastructure::{Config, Db, S3ObjectStorage};
use bikesnest_web::{RouterDeps, app_router_with};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    // Load .env from the workspace root if present (development convenience).
    dotenvy::dotenv().ok();

    // The single configuration read of the whole process: every knob below,
    // and everything the router and the worker use, comes from this value.
    let config = Config::from_env().unwrap_or_else(|err| {
        eprintln!("configuration error: {err}");
        eprintln!("hint: copy .env.example to .env and adjust values");
        std::process::exit(1);
    });

    // JSON structured logging in production is machine-parseable and forwarded to
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

    let db = Db::connect_with(&config.database_url, &config.db)
        .await
        .unwrap_or_else(|err| {
            eprintln!("database connection error: {err}");
            std::process::exit(1);
        });

    // Subcommand dispatch: default = serve the app.
    match subcommand.as_deref() {
        Some("seed-mock") => {
            // Run migrations explicitly before seeding.
            db.migrate().await.unwrap_or_else(|err| {
                eprintln!("migration error: {err}");
                std::process::exit(1);
            });
            let storage = S3ObjectStorage::from_config(&config.storage);
            let processor = bikesnest_infrastructure::LocalImageProcessor::new(config.photo);
            match bikesnest_infrastructure::parking::seed_mock(&db, &storage, &processor).await {
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
        // Idempotent admin bootstrap from the configured credentials (never HTTP).
        Some("seed-admin") => {
            db.migrate().await.unwrap_or_else(|err| {
                eprintln!("migration error: {err}");
                std::process::exit(1);
            });
            match bikesnest_infrastructure::seed_admin(&db, &config.admin_seed).await {
                Ok(bikesnest_infrastructure::auth::seed::SeedOutcome::Created) => {
                    println!("admin account created");
                }
                Ok(bikesnest_infrastructure::auth::seed::SeedOutcome::Updated) => {
                    println!("admin account updated");
                }
                Err(err) => {
                    eprintln!("seed-admin error: {err}");
                    eprintln!("hint: set ADMIN_EMAIL and ADMIN_PASSWORD in .env and retry");
                    std::process::exit(1);
                }
            }
        }
        Some("seed-full-fresh") => {
            db.migrate().await.unwrap_or_else(|err| {
                eprintln!("migration error: {err}");
                std::process::exit(1);
            });
            match seed_full_fresh(&config, &db).await {
                Ok(summary) => println!(
                    "fresh seed complete: {} old objects removed; {} parking locations + photos + reviews, admin, and policies seeded",
                    summary.deleted_objects, summary.parking_locations
                ),
                Err(err) => {
                    eprintln!("seed-full-fresh error: {err}");
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
            let retention = bikesnest_infrastructure::SqlxRetentionRepository::new(
                db.clone(),
                config.retention,
                Arc::new(storage),
            );
            let job = bikesnest_application::RetentionJob::new(
                Box::new(retention),
                Box::new(bikesnest_infrastructure::SqlxAuditLog::new(db.clone())),
                Box::new(bikesnest_infrastructure::SystemClock),
                bikesnest_application::RetentionConfig {
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
            let result = match prepare_policies(&config) {
                Ok(policies) => seed_prepared_policies(&db, &config, &policies).await,
                Err(err) => Err(err),
            };
            match result {
                Ok(()) => {
                    println!(
                        "seeded policies version {} for locales {:?} (legal text drafted by product; counsel review tracked in docs/legal-review.md)",
                        config.policy.version,
                        bikesnest_infrastructure::POLICY_LOCALES
                    );
                }
                Err(err) => {
                    eprintln!("seed-policies error: {err}");
                    std::process::exit(1);
                }
            }
        }
        _ => serve(config, db).await,
    }
}

struct PreparedPolicy {
    kind: bikesnest_domain::PolicyKind,
    kind_code: &'static str,
    locale: &'static str,
    content: String,
}

struct FullFreshSummary {
    deleted_objects: usize,
    parking_locations: usize,
}

fn prepare_policies(config: &Config) -> Result<Vec<PreparedPolicy>, String> {
    let mut policies = Vec::new();
    for &locale in bikesnest_infrastructure::POLICY_LOCALES {
        for kind_code in ["privacy", "terms", "cookies"] {
            let file = format!("policies/{kind_code}.{locale}.md");
            let raw = std::fs::read_to_string(&file)
                .map_err(|err| format!("cannot read {file}: {err}"))?;
            let content = bikesnest_infrastructure::fill_policy_placeholders(&raw, |token| {
                config.policy.placeholder(token)
            })
            .map_err(|missing| {
                let vars: Vec<&str> = bikesnest_infrastructure::POLICY_PLACEHOLDERS
                    .iter()
                    .filter(|(token, _)| missing.iter().any(|value| value == token))
                    .map(|(_, variable)| *variable)
                    .collect();
                format!(
                    "{file}: unresolved placeholders {missing:?} — set {} (see .env.example)",
                    vars.join(", ")
                )
            })?;
            policies.push(PreparedPolicy {
                kind: bikesnest_domain::PolicyKind::from_code(kind_code)
                    .expect("valid policy kind"),
                kind_code,
                locale,
                content,
            });
        }
    }
    Ok(policies)
}

async fn seed_prepared_policies(
    db: &Db,
    config: &Config,
    policies: &[PreparedPolicy],
) -> Result<(), String> {
    for policy in policies {
        bikesnest_infrastructure::seed_policy(
            db,
            policy.kind,
            policy.locale,
            &config.policy.version,
            config.policy.effective_at,
            &policy.content,
        )
        .await
        .map_err(|err| format!("{} ({}): {err}", policy.kind_code, policy.locale))?;
    }
    Ok(())
}

fn validate_admin_seed(config: &Config) -> Result<(), String> {
    let email = config
        .admin_seed
        .email
        .as_deref()
        .ok_or_else(|| "ADMIN_EMAIL must be set".to_string())?;
    bikesnest_domain::UserEmail::parse(email)
        .map_err(|_| "ADMIN_EMAIL must be a valid email".to_string())?;
    let password = config
        .admin_seed
        .password
        .as_deref()
        .ok_or_else(|| "ADMIN_PASSWORD must be set".to_string())?;
    bikesnest_domain::PasswordPolicy::default()
        .validate(password)
        .map_err(|_| "ADMIN_PASSWORD must meet the password policy".to_string())
}

async fn seed_full_fresh(config: &Config, db: &Db) -> Result<FullFreshSummary, String> {
    if config.app_env.is_production() {
        return Err("refusing to erase data when APP_ENV=production".to_string());
    }
    // Validate every file and required credential before deleting anything.
    validate_admin_seed(config)?;
    let policies = prepare_policies(config)?;

    let storage = S3ObjectStorage::from_config(&config.storage);
    let deleted_objects = bikesnest_infrastructure::reset_all_data(db, &storage)
        .await
        .map_err(|err| err.to_string())?;
    let processor = bikesnest_infrastructure::LocalImageProcessor::new(config.photo);
    let parking_locations = bikesnest_infrastructure::parking::seed_mock(db, &storage, &processor)
        .await
        .map_err(|err| err.to_string())?;
    bikesnest_infrastructure::seed_admin(db, &config.admin_seed)
        .await
        .map_err(|err| err.to_string())?;
    seed_prepared_policies(db, config, &policies).await?;

    Ok(FullFreshSummary {
        deleted_objects,
        parking_locations,
    })
}

async fn serve(config: Config, db: Db) {
    // Production validation already ran in `main` (before the database
    // connection). Development runs on fakes by design; say which ones, once.
    for fake in config.fakes_in_use() {
        tracing::warn!(component = fake, "development fake in use");
    }

    let config = Arc::new(config);

    // Run migrations explicitly on startup.
    if let Err(err) = db.migrate().await {
        eprintln!("migration error: {err}");
        std::process::exit(1);
    }
    tracing::info!(migrations = "applied", "database ready");

    let deps = RouterDeps::from_config(&config).unwrap_or_else(|err| {
        eprintln!("provider configuration error: {err}");
        std::process::exit(1);
    });

    // One shutdown signal for the whole process: SIGTERM/SIGINT cancels it, the
    // HTTP server stops accepting and drains, and the worker leaves its poll
    // loop after finishing whatever job it holds.
    let shutdown = CancellationToken::new();

    // Background worker (plans/m9-background-jobs.md): a tokio task that claims
    // and runs durable one-shot + recurring jobs. Disable with `JOBS_ENABLED=false`
    // for web-only instances (or to run jobs elsewhere).
    let worker_task = if config.jobs.enabled {
        let storage: Arc<dyn bikesnest_application::ObjectStorage> =
            Arc::new(S3ObjectStorage::from_config(&config.storage));
        // The worker gets the same provider instance the router holds, so the
        // `email.send` handler mails through the configured relay/ESP rather
        // than a second copy of it.
        let services = bikesnest_infrastructure::job_services(
            db.clone(),
            &config,
            storage,
            deps.email.clone(),
        );
        let worker =
            bikesnest_infrastructure::Worker::new(services.repo, services.registry, config.jobs);
        tracing::info!(jobs = "enabled", "background worker started");
        Some(tokio::spawn(worker.run(shutdown.child_token())))
    } else {
        tracing::info!(jobs = "disabled", "background worker not started");
        None
    };

    let bind_addr = config.bind_addr.clone();
    let app = app_router_with(config, db, deps);
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .unwrap_or_else(|err| {
            eprintln!("failed to bind {bind_addr}: {err}");
            std::process::exit(1);
        });
    tracing::info!(addr = %bind_addr, "bikesnest listening");

    // `into_make_service_with_connect_info` is what puts the TCP peer address in
    // the request extensions; without it `ClientIp` has no address to trust and
    // every caller would share one rate-limit bucket.
    let service = app.into_make_service_with_connect_info::<SocketAddr>();
    let signal = shutdown.clone();
    let server = axum::serve(listener, service).with_graceful_shutdown(async move {
        wait_for_terminate().await;
        tracing::info!("shutdown signal received; draining");
        signal.cancel();
    });
    if let Err(err) = server.await {
        tracing::error!(error = %err, "server error");
    }

    // In-flight requests are done. Give the worker a bounded grace period to
    // finish the job it may still be running rather than killing it mid-write.
    if let Some(task) = worker_task {
        match tokio::time::timeout(WORKER_SHUTDOWN_GRACE, task).await {
            Ok(Ok(())) => tracing::info!("background worker drained"),
            Ok(Err(e)) => tracing::warn!(error = %e, "background worker task failed"),
            Err(_) => tracing::warn!(
                grace_secs = WORKER_SHUTDOWN_GRACE.as_secs(),
                "background worker did not finish in time; exiting anyway"
            ),
        }
    }
    tracing::info!("bikesnest stopped cleanly");
}

/// How long an in-flight background job may take to finish once shutdown has
/// been signalled. Container runtimes typically SIGKILL ~10 s after SIGTERM, so
/// this is an upper bound, not a promise.
const WORKER_SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_secs(30);

/// Resolves on SIGTERM (what a container runtime sends) or SIGINT (Ctrl-C).
async fn wait_for_terminate() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            // Without a SIGTERM handler, Ctrl-C is the only way out; never
            // resolve here or the server would shut down immediately.
            Err(e) => {
                tracing::warn!(error = %e, "cannot install SIGTERM handler");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}
