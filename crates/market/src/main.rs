//! Runs the Orien marketplace's server.

use anyhow::Result;
use clap::Parser;
use orien_market::api::{App, router};
use orien_market::store::Store;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "orien-market", version, about = "The Orien marketplace's server")]
struct Args {
    /// The database file, made if it does not exist.
    #[arg(long, default_value = "market.db")]
    db: PathBuf,
    /// Where to listen: behind nginx, on this computer only.
    #[arg(long, default_value = "127.0.0.1:7200")]
    listen: String,
    /// The website's address, which passkeys are tied to.
    #[arg(long, default_value = "https://orien.network")]
    origin: String,
    /// Also serve the website from this folder (web/out), to try the
    /// marketplace on this computer. On the server, nginx serves the site.
    #[arg(long)]
    site: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let secure = args.origin.starts_with("https://");
    let app = Arc::new(App::new(Store::open(&args.db)?, &args.origin, secure)?);
    let listener = tokio::net::TcpListener::bind(&args.listen).await?;
    println!("marketplace server for {} listening on {}", args.origin, args.listen);
    let mut routes = router(app);
    if let Some(site) = &args.site {
        routes = routes.fallback_service(tower_http::services::ServeDir::new(site));
    }
    axum::serve(listener, routes)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
