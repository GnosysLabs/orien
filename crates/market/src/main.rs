//! Runs the Orien marketplace's server.

use anyhow::{Context, Result};
use clap::Parser;
use orien_market::api::{App, router};
use orien_market::images::Folder;
use orien_market::mail::Mailer;
use orien_market::screen::Screener;
use orien_market::wallets::Network;
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
    /// A file holding the Resend key that sends sign-in codes. Without it,
    /// codes are printed here instead: for trying it on this computer.
    #[arg(long)]
    resend_key: Option<PathBuf>,
    /// A file holding the OpenRouter key that checks listings and names
    /// before they are saved. Without it nothing is checked: for trying it
    /// on this computer.
    #[arg(long)]
    openrouter_key: Option<PathBuf>,
    /// The Orien network's genesis file, for asking its validators.
    #[arg(long, default_value = "/var/lib/orien/genesis.json")]
    genesis: PathBuf,
    /// The marketplace's own Orien address: sellers prove their wallets by
    /// paying a small amount to it.
    #[arg(long)]
    address: String,
    /// Who the codes come from.
    #[arg(long, default_value = "Orien marketplace <noreply@orien.network>")]
    from: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let secure = args.origin.starts_with("https://");
    let mailer = match &args.resend_key {
        Some(path) => {
            let key = std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
            Mailer::resend(key.trim().to_string(), &args.from)
        }
        None if secure => anyhow::bail!("a live marketplace needs --resend-key to send sign-in codes"),
        None => Mailer::log_only(),
    };
    let screener = match &args.openrouter_key {
        Some(path) => {
            let key = std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
            Screener::openrouter(key.trim().to_string())
        }
        None if secure => anyhow::bail!("a live marketplace needs --openrouter-key to check listings"),
        None => Screener::off(),
    };
    // Pictures are kept beside the database.
    let images = args.db.parent().unwrap_or(std::path::Path::new(".")).join("images");
    let payments = Arc::new(Network::open(&args.genesis)?);
    let app = Arc::new(App::new(
        Store::open(&args.db)?,
        Folder::open(&images)?,
        &args.origin,
        secure,
        mailer,
        screener,
        payments,
        &args.address,
    )?);
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
