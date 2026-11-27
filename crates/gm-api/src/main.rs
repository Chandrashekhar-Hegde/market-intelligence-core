use std::{net::SocketAddr, path::PathBuf};

use clap::Parser;
use gm_api::{ApiConfig, build_app};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Debug, Parser)]
struct Args {
    #[arg(long, env = "GM_HOST", default_value = "127.0.0.1")]
    host: String,
    #[arg(long, env = "GM_PORT", default_value_t = 8000)]
    port: u16,
    #[arg(long, env = "DATABASE_URL")]
    database_url: Option<String>,
    #[arg(long, env = "GM_MIGRATIONS", default_value = "migrations")]
    migrations: PathBuf,
    #[arg(long, env = "GM_SKIP_MIGRATIONS", default_value_t = false)]
    skip_migrations: bool,
    #[arg(long, env = "WEB_ASSETS_DIR")]
    web_assets: Option<PathBuf>,
    #[arg(long, env = "RAZORPAY_KEY_ID", default_value = "rzp_test_local")]
    razorpay_key_id: String,
    #[arg(
        long,
        env = "RAZORPAY_KEY_SECRET",
        default_value = "local_checkout_signing_key"
    )]
    razorpay_key_secret: String,
    #[arg(
        long,
        env = "RAZORPAY_WEBHOOK_SECRET",
        default_value = "local_webhook_signing_key"
    )]
    razorpay_webhook_secret: String,
    /// Shared private-beta token. Unset = open (local dev).
    #[arg(long, env = "GM_ACCESS_TOKEN", hide_env_values = true)]
    access_token: Option<String>,
    /// Comma-separated origins allowed cross-origin. Defaults cover local Vite dev; set to
    /// "none" in a same-origin deployment.
    #[arg(
        long,
        env = "GM_CORS_ORIGINS",
        default_value = "http://127.0.0.1:5173,http://localhost:5173"
    )]
    cors_origins: String,
    /// Directory for the news recorder's `news.jsonl` (a Railway volume). Unset = not recording.
    #[arg(long, env = "GM_DATA_DIR")]
    data_dir: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();
    let args = Args::parse();
    let app = build_app(ApiConfig {
        database_url: args.database_url,
        migrations: args.migrations,
        run_migrations: !args.skip_migrations,
        web_assets: args.web_assets,
        payment_key_id: args.razorpay_key_id,
        payment_checkout_secret: args.razorpay_key_secret,
        payment_webhook_secret: args.razorpay_webhook_secret,
        access_token: args.access_token,
        cors_origins: args
            .cors_origins
            .split(',')
            .map(str::trim)
            .filter(|o| !o.is_empty() && *o != "none")
            .map(String::from)
            .collect(),
        llm: gm_integrations::llm::LlmConfig::from_env(),
        data_dir: args.data_dir,
        warm_today: true,
        ..ApiConfig::default()
    })
    .await?;

    // Railway (and most PaaS) inject PORT; it wins over GM_PORT.
    let port = match std::env::var("PORT") {
        Ok(port) => port.parse()?,
        Err(_) => args.port,
    };
    let addr: SocketAddr = format!("{}:{}", args.host, port).parse()?;
    tracing::info!(%addr, "starting gm-api");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("gm_api=info,tower_http=info"));
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .init();
}
