//! Network alerts for the founder: a private #network-alerts channel that
//! only the server's owner and the bot can see, and an inbox on this
//! computer where the project's own services (the Market server first) hand
//! the bot a problem to post there. The inbox listens on 127.0.0.1 only and
//! nginx never passes anything to it.

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

/// The longest alert posted; longer ones are cut.
const MOST: usize = 1800;

struct Inbox {
    http: Arc<Http>,
    /// The channel, once found or made.
    channel: Mutex<Option<ChannelId>>,
}

/// The private channel: found by name, or made once with only the owner and
/// the bot able to see it. Says which permission is missing when Discord
/// refuses.
pub async fn channel(http: &Http) -> Result<ChannelId> {
    let guilds = http.get_guilds(None, None).await?;
    let [guild] = guilds.as_slice() else {
        bail!("the bot is in {} servers; it should be in exactly one", guilds.len());
    };
    let channels = guild.id.channels(http).await?;
    if let Some(found) = channels.values().find(|c| c.kind == ChannelType::Text && crate::bot::plain(&c.name) == CHANNEL) {
        return Ok(found.id);
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
        .create_channel(http, CreateChannel::new(CHANNEL).kind(ChannelType::Text).topic(TOPIC).permissions(overwrites))
        .await;
    match made {
        Ok(channel) => {
            eprintln!("alerts: made the private #{CHANNEL}");
            Ok(channel.id)
        }
        Err(error) if format!("{error}").contains("Missing Permissions") || format!("{error}").contains("Missing Access") => bail!(
            "Discord refused to make #{CHANNEL}: in Server Settings → Roles, give the bot's role \"Manage Channels\" and \"Manage Roles\", then restart the bot ({error})"
        ),
        Err(error) => Err(error).context(format!("cannot make #{CHANNEL}")),
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
        None => match channel(&inbox.http).await {
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

/// Makes sure the channel is there, then takes alerts on `listen` until the
/// bot stops.
pub async fn serve(http: Arc<Http>, listen: String) -> Result<()> {
    let first = match channel(&http).await {
        Ok(channel) => Some(channel),
        Err(error) => {
            eprintln!("alerts: {error:#}");
            None
        }
    };
    let inbox = Arc::new(Inbox { http, channel: Mutex::new(first) });
    let app = axum::Router::new().route("/alert", post(alert)).with_state(inbox);
    let listener = tokio::net::TcpListener::bind(&listen).await?;
    eprintln!("alerts: listening on {listen} for #{CHANNEL}");
    axum::serve(listener, app).await?;
    Ok(())
}
