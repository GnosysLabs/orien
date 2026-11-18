//! The Discord side: listening in the server, answering, the role buttons
//! in #welcome and removing recovery phrases.
//!
//! It finds its channels and roles by name: #welcome, #help, #mining, and
//! the roles Member, Miner and Validator. It never reads or sends private
//! messages: it does not ask Discord for them at all.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use anyhow::Result;
use serenity::all::{
    ButtonStyle, ChannelId, ChannelType, Client, ComponentInteraction, Context, CreateActionRow, CreateAllowedMentions,
    CreateButton, CreateInteractionResponse, CreateInteractionResponseMessage, CreateMessage, EditChannel, EditMessage,
    EventHandler,
    GatewayIntents, GetMessages, Guild, GuildId, Interaction, Message, MessageFlags, MessageType, Ready, RoleId, UserId,
};
use serenity::async_trait;
use tokio::sync::Mutex;

use crate::expert::{Expert, Question, WARNING, clip};
use crate::holders::{self, Holders};
use crate::safety;
use crate::texts;

const ACCEPT: &str = "orien:accept-rules";
const MINER: &str = "orien:toggle-miner";
const VALIDATOR: &str = "orien:toggle-validator";
const VERIFY: &str = "orien:verify-holder";

/// How long the server's channels and roles are remembered before they
/// are looked up again, so a rename needs no restart.
const REMEMBER: Duration = Duration::from_secs(300);

fn welcome_buttons() -> Vec<CreateActionRow> {
    vec![
        CreateActionRow::Buttons(vec![CreateButton::new(ACCEPT).label("I accept the rules").style(ButtonStyle::Success)]),
        CreateActionRow::Buttons(vec![
            CreateButton::new(MINER).label("Miner").emoji('⛏').style(ButtonStyle::Secondary),
            CreateButton::new(VALIDATOR).label("Validator").emoji('🛡').style(ButtonStyle::Secondary),
        ]),
        CreateActionRow::Buttons(vec![CreateButton::new(VERIFY).label("Verify my ORN").emoji('✅').style(ButtonStyle::Primary)]),
    ]
}

/// The server's channels and roles the bot works with.
struct Places {
    seen: Instant,
    welcome: Option<ChannelId>,
    help: Option<ChannelId>,
    /// Where new members are greeted.
    general: Option<ChannelId>,
    /// Channels where questions are answered without a mention.
    answers: Vec<ChannelId>,
    names: HashMap<ChannelId, String>,
    member: Option<RoleId>,
    miner: Option<RoleId>,
    validator: Option<RoleId>,
    holder: Option<RoleId>,
    /// The role Discord made for the bot itself, which people sometimes
    /// mention instead of the bot.
    own_role: Option<RoleId>,
}

impl Places {
    /// A channel's number, by its plain name.
    fn channel(&self, name: &str) -> Option<u64> {
        self.names.iter().find(|(_, plain)| plain.as_str() == name).map(|(id, _)| id.get())
    }
}

/// A channel's name without decoration: "❓・help" is "help".
pub fn plain(name: &str) -> String {
    let kept: String = name.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' }).collect();
    kept.split_whitespace().collect::<Vec<_>>().join("-")
}

/// Who has been answered lately, to keep the bill and the noise down.
struct Limits {
    per_person: usize,
    per_hour: usize,
    everyone: VecDeque<Instant>,
    people: HashMap<UserId, VecDeque<Instant>>,
    /// People already told they hit the limit, and when.
    told: HashMap<UserId, Instant>,
}

enum Allowed {
    Yes,
    /// Over the limit: say so.
    No,
    /// Over the limit, and already told.
    Silent,
}

impl Limits {
    fn check(&mut self, who: UserId) -> Allowed {
        let now = Instant::now();
        let hour = Duration::from_secs(3600);
        let fresh = |times: &mut VecDeque<Instant>| {
            while times.front().is_some_and(|t| now.duration_since(*t) > hour) {
                times.pop_front();
            }
        };
        fresh(&mut self.everyone);
        self.people.retain(|_, times| {
            fresh(times);
            !times.is_empty()
        });
        self.told.retain(|_, when| now.duration_since(*when) <= hour);
        let mine = self.people.entry(who).or_default();
        if mine.len() >= self.per_person || self.everyone.len() >= self.per_hour {
            return if self.told.insert(who, now).is_none() { Allowed::No } else { Allowed::Silent };
        }
        mine.push_back(now);
        self.everyone.push_back(now);
        Allowed::Yes
    }
}

struct Handler {
    expert: Arc<Expert>,
    /// Present when the Verified Holder role is switched on.
    holders: Option<Arc<Holders>>,
    me: OnceLock<UserId>,
    /// The one server the bot works in: the first it sees.
    home: OnceLock<GuildId>,
    places: Mutex<Option<Arc<Places>>>,
    limits: Mutex<Limits>,
}

impl Handler {
    async fn places(&self, ctx: &Context, guild: GuildId) -> Option<Arc<Places>> {
        let mut held = self.places.lock().await;
        if let Some(places) = held.as_ref()
            && places.seen.elapsed() < REMEMBER
        {
            return Some(places.clone());
        }
        match self.look_up(ctx, guild).await {
            Ok(places) => {
                let places = Arc::new(places);
                *held = Some(places.clone());
                Some(places)
            }
            Err(error) => {
                eprintln!("cannot read the server's channels and roles: {error}");
                held.clone()
            }
        }
    }

    async fn look_up(&self, ctx: &Context, guild: GuildId) -> Result<Places> {
        let channels = guild.channels(&ctx.http).await?;
        let roles = guild.roles(&ctx.http).await?;
        let text: Vec<_> = channels.values().filter(|c| matches!(c.kind, ChannelType::Text | ChannelType::News)).collect();
        let channel = |name: &str| text.iter().find(|c| plain(&c.name) == name).map(|c| c.id);
        let role = |name: &str| roles.values().find(|r| r.name.trim().eq_ignore_ascii_case(name)).map(|r| r.id);
        let me = self.me.get().copied();
        let places = Places {
            seen: Instant::now(),
            welcome: channel("welcome"),
            help: channel("help"),
            general: channel("general"),
            answers: ["help", "mining"].into_iter().filter_map(channel).collect(),
            names: text.iter().map(|c| (c.id, plain(&c.name))).collect(),
            member: role("Member"),
            miner: role("Miner"),
            validator: role("Validator"),
            holder: role(holders::ROLE),
            own_role: roles.values().find(|r| me.is_some() && r.tags.bot_id == me).map(|r| r.id),
        };
        for (what, found) in [
            ("channel #welcome", places.welcome.is_some()),
            ("channel #help", places.help.is_some()),
            ("channel #mining", places.answers.len() == 2),
            ("role Member", places.member.is_some()),
            ("role Miner", places.miner.is_some()),
            ("role Validator", places.validator.is_some()),
            ("role Verified Holder", places.holder.is_some() || self.holders.is_none()),
        ] {
            if !found {
                eprintln!("the server has no {what}");
            }
        }
        Ok(places)
    }

    /// Puts the welcome message with its buttons in #welcome, or brings the
    /// one already there up to date.
    async fn keep_welcome(&self, ctx: &Context, places: &Places) -> Result<()> {
        let (Some(welcome), Some(me)) = (places.welcome, self.me.get()) else { return Ok(()) };
        let text = texts::fill(texts::WELCOME, |name| places.channel(name));
        let recent = welcome.messages(&ctx.http, GetMessages::new().limit(50)).await?;
        match recent.iter().find(|m| m.author.id == *me && !m.components.is_empty()) {
            Some(mine) if mine.content.trim() == text.trim() && mine.components.len() == welcome_buttons().len() => {}
            Some(mine) => {
                let edit = EditMessage::new().content(text).components(welcome_buttons()).suppress_embeds(true);
                welcome.edit_message(&ctx.http, mine.id, edit).await?;
                eprintln!("brought the welcome message up to date");
            }
            None => {
                // No preview card of the website between the text and the buttons.
                let message = CreateMessage::new().content(text).components(welcome_buttons()).flags(MessageFlags::SUPPRESS_EMBEDS);
                welcome.send_message(&ctx.http, message).await?;
                eprintln!("posted the welcome message with its buttons");
            }
        }
        Ok(())
    }

    async fn answer(&self, ctx: &Context, msg: &Message, places: &Places) {
        let Some(me) = self.me.get().copied() else { return };
        let mentioned = msg.mentions.iter().any(|u| u.id == me)
            || places.own_role.is_some_and(|role| msg.mention_roles.contains(&role));
        let replied_to = msg.referenced_message.as_deref();
        let direct = mentioned || replied_to.is_some_and(|m| m.author.id == me);
        // A reply to another person is their conversation.
        let overheard = places.answers.contains(&msg.channel_id)
            && replied_to.is_none()
            && safety::looks_like_question(&msg.content);
        if !direct && !overheard {
            return;
        }
        let text = msg.content.replace(&format!("<@{me}>"), "").replace(&format!("<@!{me}>"), "");
        let text = match places.own_role {
            Some(role) => text.replace(&format!("<@&{role}>"), ""),
            None => text,
        };
        let text = text.trim();
        if text.is_empty() {
            return;
        }

        match self.limits.lock().await.check(msg.author.id) {
            Allowed::Yes => {}
            Allowed::Silent => return,
            Allowed::No => {
                // Only people who asked the bot itself are told; overheard
                // questions are simply left for humans.
                if direct {
                    let help = places.help.map(|id| format!("<#{id}>")).unwrap_or("#help".into());
                    say(ctx, msg, &format!("I've answered a lot this hour, so I'm resting for a bit. A person in {help} can help meanwhile.")).await;
                }
                return;
            }
        }

        let typing = msg.channel_id.start_typing(&ctx.http);
        // The few messages before, oldest first, so a follow-up makes sense.
        let mut before: Vec<(String, String)> = match msg.channel_id.messages(&ctx.http, GetMessages::new().before(msg.id).limit(6)).await {
            Ok(messages) => messages
                .iter()
                .rev()
                .filter(|m| !m.content.trim().is_empty())
                .map(|m| (if m.author.id == me { "you".to_string() } else { m.author.name.clone() }, m.content.clone()))
                .collect(),
            Err(_) => Vec::new(),
        };
        if let Some(replied) = replied_to
            && !replied.content.trim().is_empty()
        {
            let who = if replied.author.id == me { "you".to_string() } else { replied.author.name.clone() };
            before.push((format!("{who}, the message being replied to"), replied.content.clone()));
        }
        let channel = places.names.get(&msg.channel_id).map(String::as_str).unwrap_or("a channel");
        let question = Question { channel, direct, before: &before, author: &msg.author.name, text };
        let answer = self.expert.answer(&question).await;
        typing.stop();
        match answer {
            Ok(answer) => {
                eprintln!(
                    "#{channel} {} ({}): {} [{}]",
                    msg.author.name,
                    if direct { "asked" } else { "overheard" },
                    if answer.text.is_some() { "answered" } else { "stayed quiet" },
                    answer.usage
                );
                if let Some(text) = answer.text {
                    say(ctx, msg, &clip(&text, 1900)).await;
                }
            }
            Err(error) => {
                eprintln!("could not answer {}: {error:#}", msg.author.name);
                if direct {
                    let help = places.help.map(|id| format!("<#{id}>")).unwrap_or("#help".into());
                    say(ctx, msg, &format!("I can't answer right now. Please ask in {help} and a person will help.")).await;
                }
            }
        }
    }

    /// Greets someone who has just accepted the rules, in #general, with a
    /// hello written for them. The lines after it are always the same.
    async fn greet(&self, ctx: &Context, places: &Places, who: UserId, name: &str) {
        let Some(general) = places.general else { return };
        let hello = match self.expert.hello(name).await {
            Ok(hello) if texts::fit_hello(&hello) => hello,
            Ok(hello) => {
                eprintln!("the hello written for a new member was unfit, using the plain one: {}", clip(&hello, 200));
                texts::HELLO.to_string()
            }
            Err(error) => {
                eprintln!("could not write a hello, using the plain one: {error:#}");
                texts::HELLO.to_string()
            }
        };
        let text = format!("{hello}\n\n{}", texts::fill(texts::AFTER_HELLO, |name| places.channel(name)));
        let text = text.replace("{user}", &format!("<@{who}>"));
        let message = CreateMessage::new()
            .content(text)
            .allowed_mentions(CreateAllowedMentions::new().users([who]))
            .flags(MessageFlags::SUPPRESS_EMBEDS);
        match general.send_message(&ctx.http, message).await {
            Ok(_) => eprintln!("greeted a new member in #general"),
            Err(error) => eprintln!("could not greet a new member in #general: {error}"),
        }
    }

    async fn pressed(&self, ctx: &Context, press: &ComponentInteraction) {
        let Some(guild) = press.guild_id else { return };
        if self.home.get() != Some(&guild) {
            return;
        }
        let Some(mut places) = self.places(ctx, guild).await else { return };
        // A role made a moment ago: look at the server again before saying it is missing.
        if places.member.is_none() || places.miner.is_none() || places.validator.is_none() || places.holder.is_none() {
            *self.places.lock().await = None;
            if let Some(fresh) = self.places(ctx, guild).await {
                places = fresh;
            }
        }
        let has = |role: Option<RoleId>| role.is_some_and(|role| press.member.as_ref().is_some_and(|m| m.roles.contains(&role)));
        let help = places.help.map(|id| format!("<#{id}>")).unwrap_or("#help".into());
        let who = press.user.id;

        let mut newcomer = false;
        let reply = match press.data.custom_id.as_str() {
            ACCEPT => match places.member {
                None => "The Member role is missing from the server. Please tell the founder.".to_string(),
                Some(_) if has(places.member) => format!("You're already in. Questions go in {help}."),
                Some(role) => match ctx.http.add_member_role(guild, who, role, Some("accepted the rules")).await {
                    Ok(()) => {
                        newcomer = true;
                        format!("Welcome! The rest of the server is open to you now. Questions go in {help}.\n\n⚠️ {WARNING}")
                    }
                    Err(error) => cannot(&error, "Member"),
                },
            },
            id @ (MINER | VALIDATOR) => {
                let (name, role) = if id == MINER { ("Miner", places.miner) } else { ("Validator", places.validator) };
                match role {
                    None => format!("The {name} role is missing from the server. Please tell the founder."),
                    Some(_) if !has(places.member) => "Press **I accept the rules** first.".to_string(),
                    Some(role) if has(Some(role)) => match ctx.http.remove_member_role(guild, who, role, Some("took it off themselves")).await {
                        Ok(()) => format!("**{name}** is off."),
                        Err(error) => cannot(&error, name),
                    },
                    Some(role) => match ctx.http.add_member_role(guild, who, role, Some("picked it themselves")).await {
                        Ok(()) => format!("You're a **{name}** now. Press the button again to take it off."),
                        Err(error) => cannot(&error, name),
                    },
                }
            }
            VERIFY => match (&self.holders, places.holder) {
                (None, _) => "Verifying isn't switched on yet.".to_string(),
                (_, None) => "The Verified Holder role is missing from the server. Please tell the founder.".to_string(),
                _ if !has(places.member) => "Press **I accept the rules** first.".to_string(),
                (Some(holders), _) if has(places.holder) && holders.is_verified(who) => "You're already a **Verified Holder**.".to_string(),
                (Some(holders), _) => match holders.ask(press) {
                    None => "I can't reach the Orien network right now. Please try again in a minute.".to_string(),
                    Some(amount) => format!(
                        "To become a **Verified Holder**, send exactly\n```\n{}\n```\nORN from your Orien Wallet to\n```\n{}\n```\nwithin {} minutes. I'll give you the role a minute or two after it arrives.\n\nSending it shows the wallet is yours. I will never ask for your recovery phrase or password.",
                        holders::orn(amount),
                        holders.address,
                        holders::MINUTES
                    ),
                },
            },
            _ => return,
        };
        // Shown only to the person who pressed.
        let message = CreateInteractionResponseMessage::new().content(reply).ephemeral(true);
        if let Err(error) = press.create_response(&ctx.http, CreateInteractionResponse::Message(message)).await {
            eprintln!("could not reply to a button: {error}");
        }
        // After the button has answered: Discord allows it three seconds,
        // and writing a hello takes longer.
        if newcomer {
            let name = press
                .member
                .as_ref()
                .and_then(|m| m.nick.clone())
                .or_else(|| press.user.global_name.clone())
                .unwrap_or_else(|| press.user.name.clone());
            self.greet(ctx, &places, who, &name).await;
        }
    }
}

/// What to tell someone when Discord refuses a role change.
fn cannot(error: &serenity::Error, role: &str) -> String {
    eprintln!("could not change the {role} role: {error} (is the bot's own role above {role} in the server's role list?)");
    "I couldn't do that just now. Please tell the founder.".to_string()
}

/// Replies in the channel. Nothing in the text can ping @everyone, a role
/// or another person, whatever the model wrote.
async fn say(ctx: &Context, to: &Message, text: &str) {
    let message = CreateMessage::new()
        .content(text)
        .reference_message(to)
        .allowed_mentions(CreateAllowedMentions::new().replied_user(true));
    if let Err(error) = to.channel_id.send_message(&ctx.http, message).await {
        eprintln!("could not post in the channel: {error}");
    }
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, _ctx: Context, ready: Ready) {
        let _ = self.me.set(ready.user.id);
        eprintln!("signed in to Discord as {}", ready.user.name);
    }

    async fn guild_create(&self, ctx: Context, guild: Guild, _is_new: Option<bool>) {
        let home = *self.home.get_or_init(|| guild.id);
        if home != guild.id {
            eprintln!("ignoring another server, {}: this bot works in one", guild.name);
            return;
        }
        eprintln!("working in the server {}", guild.name);
        *self.places.lock().await = None;
        if let Some(places) = self.places(&ctx, guild.id).await
            && let Err(error) = self.keep_welcome(&ctx, &places).await
        {
            eprintln!("could not put the welcome message in #welcome: {error}");
        }
    }

    async fn message(&self, ctx: Context, msg: Message) {
        let Some(guild) = msg.guild_id else { return };
        if msg.author.bot || msg.webhook_id.is_some() || self.home.get() != Some(&guild) {
            return;
        }
        if !matches!(msg.kind, MessageType::Regular | MessageType::InlineReply) {
            return;
        }
        if safety::holds_recovery_phrase(&msg.content) {
            let removed = msg.delete(&ctx.http).await.is_ok();
            eprintln!("{} posted what looks like a recovery phrase; {}", msg.author.name, if removed { "removed" } else { "could not remove it" });
            let first = if removed {
                "I removed your message because it looked like a recovery phrase."
            } else {
                "that looks like a recovery phrase. Delete your message now."
            };
            let warning = format!(
                "<@{}> {first} Anyone who saw it can take the ORN in that wallet, so make a new wallet and send your ORN to it right away.\n\n⚠️ {WARNING}",
                msg.author.id
            );
            let message = CreateMessage::new().content(warning).allowed_mentions(CreateAllowedMentions::new().users([msg.author.id]));
            if let Err(error) = msg.channel_id.send_message(&ctx.http, message).await {
                eprintln!("could not post the warning: {error}");
            }
            return;
        }
        let Some(places) = self.places(&ctx, guild).await else { return };
        self.answer(&ctx, &msg, &places).await;
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::Component(press) = interaction {
            self.pressed(&ctx, &press).await;
        }
    }
}

pub async fn run(
    token: String,
    expert: Expert,
    per_person: usize,
    per_hour: usize,
    news: Option<(String, std::path::PathBuf)>,
    shipped: Option<String>,
    holders: Option<Holders>,
) -> Result<()> {
    let holders = holders.map(Arc::new);
    let expert = Arc::new(expert);
    if let Some(shipped) = shipped {
        tokio::spawn(crate::study::keep_current(expert.clone(), shipped));
    }
    // Servers and their messages only: no private messages, no member lists.
    let intents = GatewayIntents::GUILDS | GatewayIntents::GUILD_MESSAGES | GatewayIntents::MESSAGE_CONTENT;
    let handler = Handler {
        expert: expert.clone(),
        holders: holders.clone(),
        me: OnceLock::new(),
        home: OnceLock::new(),
        places: Mutex::new(None),
        limits: Mutex::new(Limits { per_person, per_hour, everyone: VecDeque::new(), people: HashMap::new(), told: HashMap::new() }),
    };
    let mut client = Client::builder(&token, intents).event_handler(handler).await?;
    if let Some(holders) = holders {
        tokio::spawn(holders.watch(client.http.clone()));
    }
    if let Some((url, file)) = news {
        tokio::spawn(crate::news::watch(client.http.clone(), expert, url, file));
    }
    client.start().await?;
    Ok(())
}

/// Sets each channel's about line to the one in texts.rs, or with `dry`
/// only shows what would change. Run by hand, once the founder has approved
/// the lines; the running bot never changes them.
pub async fn topics(token: String, dry: bool) -> Result<()> {
    let http = serenity::http::Http::new(&token);
    let guilds = http.get_guilds(None, None).await?;
    let [guild] = guilds.as_slice() else {
        anyhow::bail!("the bot is in {} servers; it should be in exactly one", guilds.len());
    };
    let channels = guild.id.channels(&http).await?;
    for (name, topic) in texts::TOPICS {
        let found = channels.values().find(|c| matches!(c.kind, ChannelType::Text | ChannelType::News) && plain(&c.name) == name);
        let Some(channel) = found else {
            println!("#{name}: the server has no such channel");
            continue;
        };
        if channel.topic.as_deref().unwrap_or("") == topic {
            println!("#{name}: already set");
        } else if dry {
            println!("#{name}: would change from {:?}", channel.topic.as_deref().unwrap_or(""));
        } else {
            channel.id.edit(&http, EditChannel::new().topic(topic)).await?;
            println!("#{name}: set");
        }
    }
    Ok(())
}

/// Posts each channel's introduction from texts.rs, or brings the one the
/// bot posted before up to date. Run by hand; with `dry` it only shows
/// what it would do.
pub async fn intros(token: String, dry: bool) -> Result<()> {
    let http = serenity::http::Http::new(&token);
    let me = http.get_current_user().await?.id;
    let guilds = http.get_guilds(None, None).await?;
    let [guild] = guilds.as_slice() else {
        anyhow::bail!("the bot is in {} servers; it should be in exactly one", guilds.len());
    };
    let channels = guild.id.channels(&http).await?;
    let text_channels: Vec<_> = channels.values().filter(|c| matches!(c.kind, ChannelType::Text | ChannelType::News)).collect();
    let find = |name: &str| text_channels.iter().find(|c| plain(&c.name) == name).map(|c| c.id);
    for (name, intro) in texts::INTROS {
        let Some(channel) = find(name) else {
            println!("#{name}: the server has no such channel");
            continue;
        };
        let text = texts::fill(intro, |name| find(name).map(|id| id.get()));
        let heading = text.lines().next().unwrap_or("").to_string();
        let recent = channel.messages(&http, GetMessages::new().limit(100)).await?;
        match recent.iter().find(|m| m.author.id == me && m.content.starts_with(&heading)) {
            Some(mine) if mine.content.trim() == text.trim() => println!("#{name}: already there"),
            Some(_) if dry => println!("#{name}: would bring its introduction up to date"),
            Some(mine) => {
                channel.edit_message(&http, mine.id, EditMessage::new().content(text).suppress_embeds(true)).await?;
                println!("#{name}: brought up to date");
            }
            None if dry => println!("#{name}: would post ({} messages there now)", recent.len()),
            None => {
                let message = CreateMessage::new().content(text).allowed_mentions(CreateAllowedMentions::new()).flags(MessageFlags::SUPPRESS_EMBEDS);
                channel.send_message(&http, message).await?;
                println!("#{name}: posted");
            }
        }
    }
    Ok(())
}
