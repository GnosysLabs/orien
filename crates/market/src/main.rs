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
#[command(name = "orien-market", version, about = "The server for Orien Network accounts, the Market and the swap board")]
struct Args {
    /// The database file, made if it does not exist.
    #[arg(long, default_value = "market.db")]
    db: PathBuf,
    /// Where to listen: behind nginx, on this computer only.
    #[arg(long, default_value = "127.0.0.1:7200")]
    listen: String,
    /// The website's address, for links in emails.
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
    /// A file holding the KLIPY key for GIFs in the community composer.
    /// Without it, GIFs can't be searched.
    #[arg(long)]
    klipy_key: Option<PathBuf>,
    /// The Discord bot's alert inbox, such as http://127.0.0.1:7301/alert:
    /// problems (screening failing, for one) are posted in its private
    /// #network-alerts channel. Without it, they're only written to the log.
    #[arg(long)]
    alerts: Option<String>,
    /// Where Orien AI Support is answered: the Discord bot's expert on this
    /// computer. Empty turns it off.
    #[arg(long, default_value = "http://127.0.0.1:7300/support")]
    support: String,
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
    /// Let anyone signed in register websites for Sign in with Orien.
    #[arg(long)]
    sign_in_open: bool,
    /// Until then, the account names that may, comma-separated.
    #[arg(long, value_delimiter = ',')]
    sign_in_developers: Vec<String>,
    /// Tailscale logins allowed to use the dashboard, comma-separated.
    /// Without any, everyone on the tailnet may.
    #[arg(long, value_delimiter = ',')]
    admin_logins: Vec<String>,
    /// An email for App Store review that signs in with a fixed code, never
    /// emailed; the code is read from --review-code-file.
    #[arg(long, requires = "review_code_file")]
    review_email: Option<String>,
    #[arg(long)]
    review_code_file: Option<PathBuf>,
    /// Who the codes come from.
    #[arg(long, default_value = "Orien Network <noreply@orien.network>")]
    from: String,
    /// Solana `orien-order` program ID for funded buy orders.
    #[arg(long)]
    order_program: Option<String>,
    /// Solana `orien-book` program ID for buy orders that fill in parts.
    #[arg(long)]
    book_program: Option<String>,
    /// Solana RPC endpoints for funded buy orders (comma-separated).
    #[arg(long, value_delimiter = ',')]
    solana_rpc: Vec<String>,
    /// A file holding the Helius API key the wallet's Solana requests are
    /// relayed with, at /api/market/solana.
    #[arg(long)]
    helius_key: Option<std::path::PathBuf>,
    /// Path to a Solana JSON keypair file paying tiny relay fees for `claim` /
    /// `reopen` / `expire` on funded buy orders.
    #[arg(long)]
    solana_relayer_key: Option<PathBuf>,
    /// Per-order SOL cap for funded buy orders (default 10 SOL).
    #[arg(long, default_value_t = 10.0)]
    max_order_sol: f64,
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
    /// Take an account's profile picture away (it shows the robot face),
    /// without suspending it.
    RemovePicture { account: i64 },
    /// Take an account's profile banner away, without suspending it.
    RemoveBanner { account: i64 },
    /// Show an account, by its number or the email it signs in with.
    Account { who: String },
    /// Make an account (by number or email) a group chat moderator, or with
    /// --remove stop it being one. Kept in the database, never in the code.
    Moderator {
        who: String,
        #[arg(long)]
        remove: bool,
    },
    /// Let an account (by number or email) share themes without the daily
    /// and live limits, or with --remove put the limits back. Kept in the
    /// database, never in the code.
    ThemesUnlimited {
        who: String,
        #[arg(long)]
        remove: bool,
    },
}

fn admin(store: &Store, images: &Folder, command: Admin) -> Result<()> {
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
        Admin::RemovePicture { account } => {
            if let Some(old) = store.clear_picture(account)? {
                images.remove(&old);
            }
            println!("account {account}'s picture removed");
        }
        Admin::RemoveBanner { account } => {
            if let Some(old) = store.set_banner(account, None)? {
                images.remove(&old);
            }
            println!("account {account}'s banner removed");
        }
        Admin::Unsuspend { account } => {
            store.unsuspend(account)?;
            println!("account {account} may sign in again");
        }
        Admin::Moderator { who, remove } => {
            let id = match who.parse::<i64>() {
                Ok(id) => Some(id),
                Err(_) => store.account_by_email(&who.trim().to_lowercase())?,
            };
            let account = id.and_then(|id| store.account(id).ok().flatten());
            let Some(account) = account else { anyhow::bail!("no account {who}") };
            store.room_set_moderator(account.id, !remove)?;
            println!("{} ({}) is {} a group chat moderator", account.name, account.id, if remove { "no longer" } else { "now" });
        }
        Admin::ThemesUnlimited { who, remove } => {
            let id = match who.parse::<i64>() {
                Ok(id) => Some(id),
                Err(_) => store.account_by_email(&who.trim().to_lowercase())?,
            };
            let account = id.and_then(|id| store.account(id).ok().flatten());
            let Some(account) = account else { anyhow::bail!("no account {who}") };
            store.set_theme_unlimited(account.id, !remove)?;
            println!(
                "{} ({}) {} share themes without limits",
                account.name,
                account.id,
                if remove { "no longer may" } else { "may now" }
            );
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
        let images = Folder::open(&args.db.parent().unwrap_or(std::path::Path::new(".")).join("images"))?;
        return admin(&Store::open(&args.db)?, &images, command);
    }
    let address = args.address.clone().ok_or_else(|| anyhow::anyhow!("say the marketplace's own Orien address with --address"))?;
    let secure = args.origin.starts_with("https://");
    let mailer = match &args.resend_key {
        Some(path) => {
            let key = std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
            Mailer::resend(key.trim().to_string(), &args.from)
                .keeping_usage_in(args.db.parent().unwrap_or(std::path::Path::new(".")).join("resend-usage.json"))
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
    let app = App::new(
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
    .with_min_swap_version(args.min_swap_version)
    .with_alerts(args.alerts.clone())
    .with_support(Some(args.support.clone()))
    .with_klipy_key(match &args.klipy_key {
        Some(path) => std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?.trim().to_string(),
        None => String::new(),
    })
    .with_sign_in(args.sign_in_open, args.sign_in_developers.iter().map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).collect());
    let app = app.with_solana_relay(match &args.helius_key {
        Some(path) => Some(format!(
            "https://mainnet.helius-rpc.com/?api-key={}",
            std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?.trim()
        )),
        None => None,
    });
    let app = app.with_max_order_lamports((args.max_order_sol.max(0.01) * 1e9) as u64);
    let app = match &args.order_program {
        Some(program_id) => {
            let rpc_owned: Vec<String> = if args.solana_rpc.is_empty() {
                vec!["https://api.mainnet-beta.solana.com".into(), "https://solana-rpc.publicnode.com".into()]
            } else {
                args.solana_rpc.clone()
            };
            let rpc_refs: Vec<&str> = rpc_owned.iter().map(String::as_str).collect();
            let relayer = match &args.solana_relayer_key {
                Some(path) => Some(orien_market::orders::load_relayer_keypair(path)?),
                None => None,
            };
            let chain = Arc::new(orien_market::orders::LiveOrderChain::new(&rpc_refs, program_id, relayer)?);
            println!("funded buy orders enabled for program {program_id}");
            app.with_order_chain(chain)
        }
        None => app,
    };
    let app = match &args.book_program {
        Some(program_id) => {
            let rpc_owned: Vec<String> = if args.solana_rpc.is_empty() {
                vec!["https://api.mainnet-beta.solana.com".into(), "https://solana-rpc.publicnode.com".into()]
            } else {
                args.solana_rpc.clone()
            };
            let rpc_refs: Vec<&str> = rpc_owned.iter().map(String::as_str).collect();
            let relayer = match &args.solana_relayer_key {
                Some(path) => Some(orien_market::orders::load_relayer_keypair(path)?),
                None => anyhow::bail!("--book-program needs --solana-relayer-key: it fills the parts"),
            };
            let chain = Arc::new(orien_market::books::LiveBookChain::new(&rpc_refs, program_id, relayer)?);
            println!("part-filling buy orders enabled for program {program_id}");
            app.with_book_chain(chain)
        }
        None => app,
    };
    let app = Arc::new(match (&args.review_email, &args.review_code_file) {
        (Some(email), Some(file)) => {
            let code = std::fs::read_to_string(file).with_context(|| format!("cannot read {}", file.display()))?;
            app.with_review(email, &code)?
        }
        _ => app,
    });
    let apns = match &args.apns_key {
        Some(key) => orien_market::push::Apns::load(key, &args.apns_key_id, &args.apns_team)?,
        None => {
            println!("no --apns-key: iPhone notifications go to this log instead");
            orien_market::push::Apns::off()
        }
    };
    orien_market::push::watch(app.clone(), apns);
    orien_market::community::watch(app.clone());
    orien_market::proofs::watch(app.clone());
    orien_market::orders::watch(app.clone());
    orien_market::books::watch(app.clone());
    let openrouter = match &args.openrouter_key {
        Some(path) => Some(std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?.trim().to_string()),
        None => None,
    };
    if openrouter.is_some() {
        orien_market::api::watch_screening(app.clone());
    }
    orien_market::alerts::watch(app.clone(), openrouter);
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
