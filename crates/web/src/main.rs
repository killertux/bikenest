use bikenest_infrastructure::{Config, Db};
use bikenest_web::app_router;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    // Load .env from the workspace root if present (dev convenience; §10).
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

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
            let storage = bikenest_infrastructure::LocalDiskStorage::from_env();
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
