//! The Orien community's Discord bot. It answers questions about Orien from
//! the project's own material, gives Member to people who accept the rules,
//! lets them pick Miner and Validator for themselves, and removes recovery
//! phrases posted by mistake.
//!
//!     orien-discord run --token ~/.config/orien/discord-bot.token \
//!         --openrouter-key ~/.config/orien/openrouter.key --knowledge discord-bot/knowledge.md
//!
//! To try its answers without Discord:
//!
//!     orien-discord ask --openrouter-key ~/.config/orien/openrouter.key \
//!         --knowledge discord-bot/knowledge.md "How do I start mining?"

mod alerts;
mod bot;
mod checkpoints;
mod expert;
mod holders;
mod news;
mod safety;
mod study;
mod videos;
mod texts;
mod web;

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Args)]
struct Brain {
    /// A file holding the OpenRouter key that pays for the answers.
    #[arg(long)]
    openrouter_key: PathBuf,
    /// What the bot knows, written by build-knowledge.py.
    #[arg(long)]
    knowledge: PathBuf,
    /// The model that answers, by its OpenRouter name.
    #[arg(long, default_value = "anthropic/claude-haiku-5.5")]
    model: String,
    /// The model that writes announcements.
    #[arg(long, default_value = "anthropic/claude-sonnet-5.5")]
    news_model: String,
}

#[derive(Subcommand)]
enum Command {
    /// Join the Discord server and stay there.
    Run {
        /// A file holding the bot's Discord token.
        #[arg(long)]
        token: PathBuf,
        #[command(flatten)]
        brain: Brain,
        /// How many answers one person may get in an hour.
        #[arg(long, default_value_t = 8)]
        per_person: usize,
        /// How many answers everyone together may get in an hour.
        #[arg(long, default_value_t = 120)]
        per_hour: usize,
        /// A folder the bot may write in. With it, the bot watches the
        /// changelog page and announces each new entry in #announcements,
        /// remembering there what it has announced.
        #[arg(long)]
        state: Option<PathBuf>,
        /// Announce each new changelog entry in #announcements. Off: the
        /// founder retired those posts on 2026-11-24; the website's changelog
        /// covers them.
        #[arg(long, requires = "state")]
        announce: bool,
        /// The changelog page to watch.
        #[arg(long, default_value = news::CHANGELOG)]
        changelog: String,
        /// The network's genesis file. With it and --state, people can
        /// become Verified Holders by sending a tiny amount of ORN.
        #[arg(long, requires = "state")]
        genesis: Option<PathBuf>,
        /// Where those tiny amounts are sent: the Market's wallet.
        #[arg(long, default_value = "74fa1ab82c45ebeed539b03146243ddfc6f0ccb5163f45ea87d21692ba774b7a")]
        verify_address: String,
        /// Post a summary in #checkpoints after every hourly checkpoint.
        /// Needs --state.
        #[arg(long, requires = "state")]
        checkpoints: bool,
        /// Answer only from the knowledge file, without reading the live
        /// website again every half hour.
        #[arg(long)]
        offline: bool,
        /// Also answer the website's chat bubble here, such as
        /// 127.0.0.1:7300, for nginx to pass /api/ask to.
        #[arg(long)]
        web_listen: Option<String>,
        /// Take network alerts here, such as 127.0.0.1:7301, and post them
        /// in the private #network-alerts channel. Never behind nginx.
        #[arg(long)]
        alerts_listen: Option<String>,
        /// How many website answers one visitor may get in an hour.
        #[arg(long, default_value_t = 20)]
        web_per_visitor: usize,
        /// How many website answers everyone together may get in a day.
        #[arg(long, default_value_t = 1500)]
        web_per_day: usize,
    },
    /// Answer only the website's chat, without Discord: for trying it on
    /// this computer.
    Web {
        #[command(flatten)]
        brain: Brain,
        #[arg(long, default_value = "127.0.0.1:7300")]
        listen: String,
    },
    /// Ask the network what the Verified Holder check would see now.
    Network {
        #[arg(long)]
        genesis: PathBuf,
        #[arg(long, default_value = "74fa1ab82c45ebeed539b03146243ddfc6f0ccb5163f45ea87d21692ba774b7a")]
        verify_address: String,
        /// A wallet address to look at.
        wallet: Option<String>,
    },
    /// Print the summary of the checkpoint that closed last, as the bot
    /// would post it in #checkpoints. Takes a few minutes; posts nothing.
    Checkpoint,
    /// Show what the bot would know right now from the live website.
    Study {
        /// The shipped knowledge file, for the parts not on the website.
        #[arg(long)]
        knowledge: PathBuf,
    },
    /// The one-time catch-up in #announcements: one post telling the story
    /// of the older entries, then one post for each of the newest. First
    /// `--draft FILE` writes the posts to a file to read; then `--post FILE`
    /// posts exactly what the file holds.
    CatchUp {
        #[command(flatten)]
        brain: Brain,
        /// Write the posts to this file.
        #[arg(long, conflicts_with = "post")]
        draft: Option<PathBuf>,
        /// Post what this file holds.
        #[arg(long, requires = "token")]
        post: Option<PathBuf>,
        /// A file holding the bot's Discord token, for --post.
        #[arg(long)]
        token: Option<PathBuf>,
        /// How many of the newest entries get a post of their own.
        #[arg(long, default_value_t = 5)]
        newest: usize,
        #[arg(long, default_value = news::CHANGELOG)]
        changelog: String,
    },
    /// Set each channel's about line to the approved one. Done by hand,
    /// once; the running bot never changes them.
    Topics {
        /// A file holding the bot's Discord token.
        #[arg(long)]
        token: PathBuf,
        /// Only show what would change.
        #[arg(long)]
        dry_run: bool,
    },
    /// Post each channel's introduction, or bring it up to date. Done by
    /// hand; the running bot never posts them.
    Intros {
        /// A file holding the bot's Discord token.
        #[arg(long)]
        token: PathBuf,
        /// Only show what would be posted or changed.
        #[arg(long)]
        dry_run: bool,
    },
    /// Write the hello a new member with this name would get.
    Hello {
        #[command(flatten)]
        brain: Brain,
        name: String,
    },
    /// Answer one question here, as the bot would in #help.
    Ask {
        #[command(flatten)]
        brain: Brain,
        /// Ask as if overheard in the channel, not addressed to the bot.
        #[arg(long)]
        overheard: bool,
        question: String,
    },
}

fn secret(path: &PathBuf) -> Result<String> {
    let text = std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    Ok(text.trim().to_string())
}

fn expert(brain: &Brain) -> Result<expert::Expert> {
    let knowledge = std::fs::read_to_string(&brain.knowledge)
        .with_context(|| format!("cannot read {}", brain.knowledge.display()))?;
    expert::Expert::new(secret(&brain.openrouter_key)?, brain.model.clone(), brain.news_model.clone(), &knowledge)
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Run {
            token,
            brain,
            per_person,
            per_hour,
            state,
            changelog,
            offline,
            genesis,
            verify_address,
            web_listen,
            alerts_listen,
            web_per_visitor,
            web_per_day,
            checkpoints,
            announce,
        } => {
            let holders = match (&genesis, &state) {
                (Some(genesis), Some(folder)) => Some(holders::Holders::open(genesis, &verify_address, folder.join("holders.json"))?),
                _ => None,
            };
            let news = state.map(|folder| (changelog, folder.join("announced.json")));
            let shipped = if offline { None } else { Some(std::fs::read_to_string(&brain.knowledge)?) };
            let expert = std::sync::Arc::new(expert(&brain)?);
            if let Some(listen) = web_listen {
                let expert = expert.clone();
                tokio::spawn(async move {
                    if let Err(error) = web::serve(expert, listen, web_per_visitor, web_per_day).await {
                        eprintln!("website chat stopped: {error:#}");
                    }
                });
            }
            let token = secret(&token)?;
            if let Some(listen) = alerts_listen {
                let http = std::sync::Arc::new(serenity::http::Http::new(&token));
                tokio::spawn(async move {
                    if let Err(error) = alerts::serve(http, listen).await {
                        eprintln!("alerts stopped: {error:#}");
                    }
                });
            }
            bot::run(token, expert, per_person, per_hour, news, shipped, holders, checkpoints, announce).await
        }
        Command::Web { brain, listen } => {
            let expert = std::sync::Arc::new(expert(&brain)?);
            let shipped = std::fs::read_to_string(&brain.knowledge)?;
            tokio::spawn(study::keep_current(expert.clone(), shipped));
            web::serve(expert, listen, 20, 1500).await
        }
        Command::Network { genesis, verify_address, wallet } => {
            let holders = holders::Holders::open(&genesis, &verify_address, std::env::temp_dir().join("orien-holders-unused.json"))?;
            println!("{}", tokio::task::spawn_blocking(move || holders.report(wallet.as_deref())).await?);
            Ok(())
        }
        Command::Checkpoint => {
            println!("{}", checkpoints::preview().await?);
            Ok(())
        }
        Command::Study { knowledge } => {
            let shipped = std::fs::read_to_string(&knowledge)?;
            println!("{}", study::gather(&reqwest::Client::new(), &shipped).await?);
            Ok(())
        }
        Command::CatchUp { brain, draft, post, token, newest, changelog } => {
            if let Some(file) = draft {
                let expert = expert(&brain)?;
                let entries = news::read_page(&reqwest::Client::new(), &changelog).await?;
                let (recent, older) = entries.split_at(newest.min(entries.len()));
                let mut posts = Vec::new();
                if let (Some(first), Some(last)) = (older.last(), older.first()) {
                    let story = expert.story(older).await?;
                    posts.push(format!(
                        "## 📜 The story so far\n**{} changes**, {} to {}\n\n{story}\n\nFull notes: <{}>",
                        older.len(),
                        first.day,
                        last.day,
                        news::CHANGELOG
                    ));
                }
                // Oldest first, so the newest ends up at the bottom of the channel.
                for entry in recent.iter().rev() {
                    let written = expert.announce(entry).await?;
                    let (emoji, body) = news::emoji_and_body(&written);
                    posts.push(entry.post(emoji, body));
                }
                for post in &posts {
                    anyhow::ensure!(post.chars().count() <= 2000, "a post is over Discord's 2,000 characters:\n{post}");
                }
                std::fs::write(&file, serde_json::to_string_pretty(&posts)?)?;
                println!("{}", posts.join("\n\n────────\n\n"));
                Ok(())
            } else if let (Some(file), Some(token)) = (post, token) {
                let posts: Vec<String> = serde_json::from_str(&std::fs::read_to_string(&file)?)?;
                let http = serenity::http::Http::new(&secret(&token)?);
                let channel = news::announcements(&http).await?;
                for post in &posts {
                    news::publish(&http, channel, post).await?;
                    println!("posted: {}", post.lines().next().unwrap_or(""));
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
                Ok(())
            } else {
                anyhow::bail!("give --draft FILE, or --post FILE with --token")
            }
        }
        Command::Topics { token, dry_run } => bot::topics(secret(&token)?, dry_run).await,
        Command::Intros { token, dry_run } => bot::intros(secret(&token)?, dry_run).await,
        Command::Hello { brain, name } => {
            let hello = expert(&brain)?.hello(&name).await?;
            println!("{}{hello}", if texts::fit_hello(&hello) { "" } else { "(UNFIT, the plain hello is used instead) " });
            Ok(())
        }
        Command::Ask { brain, overheard, question } => {
            let expert = expert(&brain)?;
            let shipped = std::fs::read_to_string(&brain.knowledge)?;
            match study::gather(&reqwest::Client::new(), &shipped).await {
                Ok(fresh) => expert.learn(&fresh),
                Err(error) => eprintln!("[answering from the knowledge file alone: {error:#}]"),
            }
            let answer = expert
                .answer(&expert::Question { channel: "help", direct: !overheard, before: &[], author: "someone", text: &question })
                .await?;
            println!("{}", answer.text.as_deref().unwrap_or("(stays quiet)"));
            eprintln!("[{}]", answer.usage);
            Ok(())
        }
    }
}
