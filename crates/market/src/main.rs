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
#[command(name = "orien-market", version, about = "The Orien Market's server")]
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
    /// Apple's push key (.p8), for notifications on iPhone; without it,
    /// notifications are written to the log
    #[arg(long)]
    apns_key: Option<PathBuf>,
    /// The Apple push key's id, as Apple shows it
    #[arg(long, default_value = "")]
    apns_key_id: String,
    /// The Apple developer team the key belongs to
    #[arg(long, default_value = "4PDUNTF69S")]
    apns_team: String,
    /// Turn away takes from wallets on swap rules older than this (2 for
    /// Orien Wallet 1.8.0's shorter locks); 0 lets any wallet take
    #[arg(long, default_value_t = 0)]
    min_swap_version: u32,
    /// The Orien network's genesis file, for asking its validators.
    #[arg(long, default_value = "/var/lib/orien/genesis.json")]
    genesis: PathBuf,
    /// The marketplace's own Orien address: sellers prove their wallets by
    /// paying a small amount to it. Needed to run the server.
    #[arg(long)]
    address: Option<String>,
    /// Also serve the founder's dashboard here, such as 127.0.0.1:7201, for
    /// `tailscale serve` to put on the tailnet. Never behind the public site.
    #[arg(long)]
    admin_listen: Option<String>,
    /// The website's folder, for the dashboard's page (/admin/).
    #[arg(long)]
    admin_site: Option<PathBuf>,
    /// Tailscale logins allowed to use the dashboard, comma-separated.
    /// Without any, everyone on the tailnet may.
    #[arg(long, value_delimiter = ',')]
    admin_logins: Vec<String>,
    /// Who the codes come from.
    #[arg(long, default_value = "Orien Market <noreply@orien.network>")]
    from: String,
    /// Look after the marketplace instead of running it.
    #[command(subcommand)]
    admin: Option<Admin>,
}

/// For the founder, on the server, beside the running marketplace.
#[derive(clap::Subcommand)]
enum Admin {
    /// List reports nobody has dealt with yet.
    Reports,
    /// Mark a report dealt with, without taking anything down.
    Handled { report: i64 },
    /// Take a listing down for good.
    RemoveListing {
        listing: i64,
        #[arg(long, default_value = "")]
        reason: String,
    },
    /// Suspend an account: signed out everywhere, its listings and offers
    /// taken down, its profile hidden.
    Suspend {
        account: i64,
        #[arg(long, default_value = "")]
        reason: String,
    },
    /// Lift a suspension.
    Unsuspend { account: i64 },
    /// Show an account, by its number or the email it signs in with.
    Account { who: String },
}

fn admin(store: &Store, command: Admin) -> Result<()> {
    match command {
        Admin::Reports => {
            let reports = store.open_reports()?;
            if reports.is_empty() {
                println!("no reports waiting");
            }
            for r in reports {
                let by = r.reporter_name.unwrap_or_else(|| "someone gone".into());
                println!("report {}: {} {} \"{}\", reported by {by}", r.id, r.kind, r.target, r.about);
                if !r.reason.is_empty() {
                    println!("    why: {}", r.reason);
                }
            }
        }
        Admin::Handled { report } => {
            store.mark_handled(report)?;
            println!("report {report} marked dealt with");
        }
        Admin::RemoveListing { listing, reason } => {
            store.remove_listing(listing, &reason)?;
            println!("listing {listing} taken down");
        }
        Admin::Suspend { account, reason } => {
            store.suspend(account, &reason)?;
            println!("account {account} suspended");
        }
        Admin::Unsuspend { account } => {
            store.unsuspend(account)?;
            println!("account {account} may sign in again");
        }
        Admin::Account { who } => {
            let id = match who.parse::<i64>() {
                Ok(id) => Some(id),
                Err(_) => store.account_by_email(&who.trim().to_lowercase())?,
            };
            let account = id.and_then(|id| store.account(id).ok().flatten());
            let Some(account) = account else { anyhow::bail!("no account {who}") };
            println!("account {}: {}", account.id, account.name);
            println!("  email:     {}", store.email_of(account.id)?.unwrap_or_default());
            println!("  wallet:    {}", account.wallet.unwrap_or_else(|| "not linked".into()));
            println!("  suspended: {}", if store.is_suspended(account.id)? { "yes" } else { "no" });
            println!("  listings:  {}", store.listings_of(account.id)?.len());
            println!("  swaps done: {}", store.swaps_done(account.id)?);
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    if let Some(command) = args.admin {
        return admin(&Store::open(&args.db)?, command);
    }
    let address = args.address.clone().ok_or_else(|| anyhow::anyhow!("say the marketplace's own Orien address with --address"))?;
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
    // So are the website's traffic counts.
    let traffic = args.db.parent().unwrap_or(std::path::Path::new(".")).join("traffic");
    let payments = Arc::new(Network::open(&args.genesis)?);
    let app = Arc::new(App::new(
        Store::open(&args.db)?,
        Folder::open(&images)?,
        &args.origin,
        secure,
        mailer,
        screener,
        payments,
        &address,
    )?
    .with_traffic(orien_market::traffic::Traffic::open(traffic))
    .with_min_swap_version(args.min_swap_version));
    let apns = match &args.apns_key {
        Some(key) => orien_market::push::Apns::load(key, &args.apns_key_id, &args.apns_team)?,
        None => {
            println!("no --apns-key: iPhone notifications go to this log instead");
            orien_market::push::Apns::off()
        }
    };
    orien_market::push::watch(app.clone(), apns);
    orien_market::proofs::watch(app.clone());
    let listener = tokio::net::TcpListener::bind(&args.listen).await?;
    println!("marketplace server for {} listening on {}", args.origin, args.listen);
    if let Some(at) = &args.admin_listen {
        let admins = orien_market::admin::Admins(args.admin_logins.iter().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect());
        let routes = orien_market::admin::admin_router(app.clone(), admins, args.admin_site.as_deref());
        let listener = tokio::net::TcpListener::bind(at).await?;
        println!("dashboard listening on {at}");
        tokio::spawn(async move {
            if let Err(error) = axum::serve(listener, routes).await {
                eprintln!("the dashboard stopped: {error}");
            }
        });
    }
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
