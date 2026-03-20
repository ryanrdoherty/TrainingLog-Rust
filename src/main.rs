use anyhow::Result;
use sports_log::{config::Config, db, metrics as app_metrics, routes::app_router};
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "sports_log=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = Config::from_env()?;
    let pool = db::create_pool(&config.database_url).await?;

    sqlx::migrate!("./migrations").run(&pool).await?;

    // Initialize Prometheus metrics before building the router
    let (metric_layer, handle) = app_metrics::init();
    app_metrics::spawn_process_collector();

    let app = app_router(pool, config, handle, metric_layer)
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive());

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    tracing::info!("Listening on http://0.0.0.0:3000");
    axum::serve(listener, app).await?;

    Ok(())
}
