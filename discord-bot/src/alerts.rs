//! Network alerts for the founder: a private #network-alerts channel that
//! only the server's owner and the bot can see, and an inbox on this
//! computer where the project's own services (the Market server first) hand
//! the bot a problem to post there. Problem reports from Orien Wallet go
//! the same way to a second private channel, #app-reports. The inbox
//! listens on 127.0.0.1 only and nginx never passes anything to it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use serde::Deserialize;
use serenity::all::{
    ChannelId, ChannelType, CreateAllowedMentions, CreateChannel, CreateMessage, MessageFlags, PermissionOverwrite,
    PermissionOverwriteType, Permissions, RoleId,
};
use serenity::http::Http;
use tokio::sync::Mutex;

/// The channel's name, as Discord shows it.
pub const CHANNEL: &str = "network-alerts";

/// What the channel says it is for.
const TOPIC: &str = "Problems with the Orien network and its services, posted by the bot. Only the server's owner can see this channel.";

/// The channel for problem reports from Orien Wallet, and what it is for.
pub const REPORTS: &str = "app-reports";
const REPORTS_TOPIC: &str =
    "Problem reports people send from Orien Wallet, posted by the bot. Only the server's owner can see this channel.";

/// The longest alert posted; longer ones are cut.
const MOST: usize = 1800;

struct Inbox {
    http: Arc<Http>,
    /// The bot's state folder, where each channel's id is kept so a
    /// renamed channel is still found.
    state: Option<PathBuf>,
    /// The alerts channel, once found or made.
    channel: Mutex<Option<ChannelId>>,
    /// The problem reports channel, once found or made.
    reports: Mutex<Option<ChannelId>>,
}

/// The private alerts channel.
pub async fn channel(http: &Http, state: Option<&Path>) -> Result<ChannelId> {
    private_channel(http, CHANNEL, TOPIC, state).await
}

/// Where a private channel's id is kept, in the bot's state folder.
fn kept_id(state: Option<&Path>, name: &str) -> Option<PathBuf> {
    state.map(|dir| dir.join(format!("channel-{name}")))
}

/// A private channel: the one whose id was kept, so renaming it in Discord
/// changes nothing; else found by name (emoji and symbols aside) or by its
/// topic; else made once with only the owner and the bot able to see it.
/// Its id is kept for next time. Says which permission is missing when
/// Discord refuses.
async fn private_channel(http: &Http, name: &str, topic: &str, state: Option<&Path>) -> Result<ChannelId> {
    let guilds = http.get_guilds(None, None).await?;
    let [guild] = guilds.as_slice() else {
        bail!("the bot is in {} servers; it should be in exactly one", guilds.len());
    };
    let channels = guild.id.channels(http).await?;
    let file = kept_id(state, name);
    let keep = |id: ChannelId| {
        if let Some(file) = &file
            && let Err(error) = std::fs::write(file, id.get().to_string())
        {
            eprintln!("alerts: cannot keep #{name}'s id: {error}");
        }
        id
    };
    let kept = file.as_ref().and_then(|f| std::fs::read_to_string(f).ok()).and_then(|t| t.trim().parse::<u64>().ok());
    if let Some(id) = kept.map(ChannelId::new)
        && channels.contains_key(&id)
    {
        return Ok(id);
    }
    let text = |c: &&serenity::all::GuildChannel| c.kind == ChannelType::Text;
    if let Some(found) = channels.values().filter(text).find(|c| crate::bot::plain(&c.name) == name) {
        return Ok(keep(found.id));
    }
    if let Some(found) = channels.values().filter(text).find(|c| c.topic.as_deref() == Some(topic)) {
        return Ok(keep(found.id));
    }
    let owner = http.get_guild(guild.id).await?.owner_id;
    let me = http.get_current_user().await?.id;
    let see = Permissions::VIEW_CHANNEL | Permissions::READ_MESSAGE_HISTORY;
    let overwrites = vec![
        // @everyone's role has the server's own id.
        PermissionOverwrite { allow: Permissions::empty(), deny: Permissions::VIEW_CHANNEL, kind: PermissionOverwriteType::Role(RoleId::new(guild.id.get())) },
        PermissionOverwrite { allow: see, deny: Permissions::empty(), kind: PermissionOverwriteType::Member(owner) },
        PermissionOverwrite {
            allow: see | Permissions::SEND_MESSAGES,
            deny: Permissions::empty(),
            kind: PermissionOverwriteType::Member(me),
        },
    ];
    let made = guild
        .id
        .create_channel(http, CreateChannel::new(name).kind(ChannelType::Text).topic(topic).permissions(overwrites))
        .await;
    match made {
        Ok(channel) => {
            eprintln!("alerts: made the private #{name}");
            Ok(keep(channel.id))
        }
        Err(error) if format!("{error}").contains("Missing Permissions") || format!("{error}").contains("Missing Access") => bail!(
            "Discord refused to make #{name}: in Server Settings → Roles, give the bot's role \"Manage Channels\" and \"Manage Roles\", then restart the bot ({error})"
        ),
        Err(error) => Err(error).context(format!("cannot make #{name}")),
    }
}

#[derive(Deserialize)]
struct Alert {
    text: String,
}

async fn alert(State(inbox): State<Arc<Inbox>>, Json(alert): Json<Alert>) -> Response {
    let text: String = alert.text.trim().chars().take(MOST).collect();
    if text.is_empty() {
        return (StatusCode::BAD_REQUEST, "an alert needs words").into_response();
    }
    let mut known = inbox.channel.lock().await;
    let channel = match *known {
        Some(channel) => channel,
        None => match channel(&inbox.http, inbox.state.as_deref()).await {
            Ok(channel) => *known.insert(channel),
            Err(error) => {
                eprintln!("alerts: {error:#}");
                return (StatusCode::BAD_GATEWAY, format!("{error:#}")).into_response();
            }
        },
    };
    drop(known);
    // Nothing in an alert can ping anyone, and links show no card.
    let message = CreateMessage::new().content(&text).allowed_mentions(CreateAllowedMentions::new()).flags(MessageFlags::SUPPRESS_EMBEDS);
    match channel.send_message(&inbox.http, message).await {
        Ok(_) => {
            eprintln!("alerts: posted: {}", text.lines().next().unwrap_or(""));
            StatusCode::NO_CONTENT.into_response()
        }
        Err(error) => {
            // The channel may have been deleted: find or make it again next time.
            *inbox.channel.lock().await = None;
            eprintln!("alerts: could not post: {error}");
            (StatusCode::BAD_GATEWAY, format!("could not post: {error}")).into_response()
        }
    }
}

/// A problem report from Orien Wallet, for #app-reports. Its words are
/// never written to the log.
async fn report(State(inbox): State<Arc<Inbox>>, Json(alert): Json<Alert>) -> Response {
    let text: String = alert.text.trim().chars().take(MOST).collect();
    if text.is_empty() {
        return (StatusCode::BAD_REQUEST, "a report needs words").into_response();
    }
    let mut known = inbox.reports.lock().await;
    let channel = match *known {
        Some(channel) => channel,
        None => match private_channel(&inbox.http, REPORTS, REPORTS_TOPIC, inbox.state.as_deref()).await {
            Ok(channel) => *known.insert(channel),
            Err(error) => {
                eprintln!("reports: {error:#}");
                return (StatusCode::BAD_GATEWAY, format!("{error:#}")).into_response();
            }
        },
    };
    drop(known);
    let message = CreateMessage::new().content(&text).allowed_mentions(CreateAllowedMentions::new()).flags(MessageFlags::SUPPRESS_EMBEDS);
    match channel.send_message(&inbox.http, message).await {
        Ok(_) => {
            eprintln!("reports: posted one");
            StatusCode::NO_CONTENT.into_response()
        }
        Err(error) => {
            *inbox.reports.lock().await = None;
            eprintln!("reports: could not post: {error}");
            (StatusCode::BAD_GATEWAY, format!("could not post: {error}")).into_response()
        }
    }
}

/// Makes sure the channel is there, then takes alerts on `listen` until the
/// bot stops.
pub async fn serve(http: Arc<Http>, listen: String, state: Option<PathBuf>) -> Result<()> {
    let first = match channel(&http, state.as_deref()).await {
        Ok(channel) => Some(channel),
        Err(error) => {
            eprintln!("alerts: {error:#}");
            None
        }
    };
    let inbox = Arc::new(Inbox { http, state, channel: Mutex::new(first), reports: Mutex::new(None) });
    let app = axum::Router::new().route("/alert", post(alert)).route("/report", post(report)).with_state(inbox);
    let listener = tokio::net::TcpListener::bind(&listen).await?;
    eprintln!("alerts: listening on {listen} for #{CHANNEL}");
    axum::serve(listener, app).await?;
    Ok(())
}
