use bikenest_infrastructure::{Config, Db};
use bikenest_web::app_router;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    // Load .env from the workspace root if present (dev convenience; §10).
    dotenvy::dotenv().ok();

    // Logging (§86): JSON structured in production (machine-parseable, forwarded to
    // a log driver/aggregator), human-readable in dev. `RUST_LOG` overrides the level.
    let app_env = std::env::var("APP_ENV").unwrap_or_else(|_| "development".to_string());
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    if app_env == "production" {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .json()
            .with_current_span(true)
            .with_span_list(true)
            .init();
    } else {
        tracing_subscriber::fmt().with_env_filter(filter).init();
    }

    let config = Config::from_env().unwrap_or_else(|err| {
        eprintln!("configuration error: {err}");
        eprintln!("hint: copy .env.example to .env and adjust values");
        std::process::exit(1);
    });

    let db = Db::connect(&config.database_url)
        .await
        .unwrap_or_else(|err| {
            eprintln!("database connection error: {err}");
            std::process::exit(1);
        });

    // Subcommand dispatch: default = serve the app.
    match std::env::args().nth(1).as_deref() {
        Some("seed-mock") => {
            // Explicit, reproducible migrations first (§10).
            db.migrate().await.unwrap_or_else(|err| {
                eprintln!("migration error: {err}");
                std::process::exit(1);
            });
            let storage = bikenest_infrastructure::storage_from_env();
            match bikenest_infrastructure::parking::seed_mock(&db, &storage).await {
                Ok(n) => {
                    println!("seeded {n} mock parking locations + photos (Ledger #1/#7, dev only)");
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
            let media_root = std::env::var("MEDIA_ROOT")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|_| std::path::PathBuf::from("media"));
            let storage = bikenest_infrastructure::storage_from_env();
            let retention = bikenest_infrastructure::SqlxRetentionRepository::new(
                db.clone(),
                config.retention.clone(),
                Box::new(storage),
                media_root,
            );
            let job = bikenest_application::RetentionJob::new(
                Box::new(retention),
                Box::new(bikenest_infrastructure::SqlxAuditLog::new(db.clone())),
                Box::new(bikenest_infrastructure::SystemClock),
                bikenest_application::RetentionConfig {
                    inactive_account_anonymize_after_days:
                        config.inactive_account_anonymize_after_days,
                    deleted_account_purge_after_days:
                        config.deleted_account_purge_after_days,
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
            let version = std::env::var("POLICY_VERSION")
                .unwrap_or_else(|_| "2025-01-01.1".to_string());
            let effective_at = std::env::var("POLICY_EFFECTIVE_AT")
                .ok()
                .and_then(|v| {
                    chrono::DateTime::parse_from_rfc3339(&v)
                        .ok()
                        .map(|d| d.with_timezone(&chrono::Utc))
                })
                .unwrap_or_else(chrono::Utc::now);
            let mut ok = true;
            for (kind_code, file) in [
                ("privacy", "policies/privacy.md"),
                ("terms", "policies/terms.md"),
                ("cookies", "policies/cookies.md"),
            ] {
                let content = match std::fs::read_to_string(file) {
                    Ok(c) => c,
                    Err(err) => {
                        eprintln!("seed-policies: cannot read {file}: {err}");
                        ok = false;
                        continue;
                    }
                };
                let kind = bikenest_domain::PolicyKind::from_code(kind_code)
                    .expect("valid policy kind");
                if let Err(err) = bikenest_infrastructure::seed_policy(
                    &db,
                    kind,
                    &version,
                    effective_at,
                    &content,
                )
                .await
                {
                    eprintln!("seed-policies: {kind_code}: {err}");
                    ok = false;
                }
            }
            if ok {
                println!(
                    "seeded policies as version {version} (placeholder legal text, §71 — requires review)"
                );
            } else {
                std::process::exit(1);
            }
        }
        _ => serve(config, db).await,
    }
}

async fn serve(config: Config, db: Db) {
    // Explicit, reproducible migrations on startup (dev workflow; §10).
    if let Err(err) = db.migrate().await {
        eprintln!("migration error: {err}");
        std::process::exit(1);
    }
    tracing::info!(migrations = "applied", "database ready");

    let app = app_router(db, config.probe_timeout);
    let listener = tokio::net::TcpListener::bind(&config.bind_addr)
        .await
        .unwrap_or_else(|err| {
            eprintln!("failed to bind {}: {err}", config.bind_addr);
            std::process::exit(1);
        });
    tracing::info!(addr = %config.bind_addr, "bikenest listening");
    axum::serve(listener, app).await.expect("server error");
}
