//! The words the bot posts in the server for the founder: the welcome
//! message in #welcome and each channel's about line.

/// The welcome message. `{#name}` becomes a link to that channel. The bot
/// keeps the message in #welcome the same as this text, so changing it here
/// and restarting the bot changes it there. Discord allows 2,000 characters.
pub const WELCOME: &str = "\
# Welcome to Orien ☀️

Orien (ORN) is a cryptocurrency built for the quantum age. Payments are final in a fraction of a second, every signature is designed to stay safe when quantum computers arrive, and every coin in existence was mined by someone like you.

## Start in 3 steps
1. Download Orien Wallet for Mac, Windows or Linux at https://orien.network
2. Create your wallet and write down your recovery phrase
3. Press **Mine**, then **Start mining**, and your computer starts earning ORN

## ⚠️ Stay safe
- **The team will never message you first.** Anyone who does is a scammer.
- **Staff only reply to private messages from Verified Holders**, and will never ask for your recovery phrase.
- **Never share your recovery phrase with anyone**, for any reason.
- Help happens in public, in {#help}.

## Rules
1. Be kind. No harassment or hate.
2. No spam, no ads, no begging.
3. No scams and no impersonating the team.
4. No price promises or investment advice.
5. Keep channels on topic.

## 📺 Orien on YouTube
Explainers, mining guides and wallet walkthroughs: https://www.youtube.com/@OrienNetwork

## Where to go
- {#announcements}: new releases and network news
- {#greetings}: a hello for everyone who joins
- {#general}: say hi
- {#x-posts}: our posts on X, as they go out
- {#youtube}: our videos, as they come out
- {#checkpoints}: a summary of every hour's mining
- {#themes}: show off your wallet themes
- {#mining}: setups and questions about mining
- {#swaps-market}: swap ORN for SOL or Bitcoin, and share Market listings
- {#help}: stuck? Ask here
- {#validators}: staking and running a validator
- {#bugs-and-ideas}: tell us what's broken or missing

Press **I accept the rules** to open the rest of the server. Mining? Press **Miner** to show it, and again to take it off. Hold ORN? Press **Verify my ORN** to become a **Verified Holder**.";

/// The hello posted in #greetings when someone accepts the rules, used when
/// the model's own hello for that person is missing or unfit. `{user}`
/// becomes a mention of the new member.
pub const HELLO: &str = "👋 Welcome to Orien, {user}! Say hi, everyone.";

/// What follows every hello, whoever wrote it: where to begin, and the
/// warning. `{#name}` becomes a link to that channel.
pub const AFTER_HELLO: &str = "\
New here? Download Orien Wallet at https://orien.network and press **Mine** to start earning ORN. Questions go in {#help}.

⚠️ The team will never message you first. Never share your recovery phrase with anyone.";

/// Whether a hello the model wrote is fit to post: one short line that
/// mentions the person once and carries no link, mention, channel or price.
pub fn fit_hello(hello: &str) -> bool {
    let lower = hello.to_lowercase();
    hello.matches("{user}").count() == 1
        && hello.chars().count() <= 260
        && !hello.contains('\n')
        && !["http", "www.", "discord.gg", "@", "#", "<", ">", "$", "€", "price", "invest", "profit", "recovery phrase", "airdrop", "giveaway"]
            .iter()
            .any(|banned| lower.contains(banned))
}

/// Each channel's about line, shown at the top of the channel. Set once
/// with `orien-discord topics`, never on its own.
pub const TOPICS: [(&str, &str); 13] = [
    ("welcome", "Start here: what Orien is, how to begin, and the rules."),
    ("announcements", "New releases and network news from the team."),
    ("general", "Say hi and talk about anything Orien."),
    ("mining", "Mining setups, your share of the hourly ORN, and \"is my miner working?\""),
    (
        "swaps-market",
        "Find someone to swap ORN for SOL or Bitcoin with, and share your Market listings. The team will never message you first.",
    ),
    ("help", "Stuck? Ask here. All help happens in public; never share your recovery phrase."),
    ("validators", "Staking, running a validator, and the orien command line."),
    ("bugs-and-ideas", "Tell us what's broken or what's missing."),
    ("greetings", "A hello for everyone who joins. Say hi back!"),
    ("x-posts", "Orien's posts on X, as they go out. Like and repost to help spread the word."),
    ("youtube", "Orien's videos, as they come out. Watch them right here."),
    ("checkpoints", "A summary after every hourly checkpoint: who mined, how the hour's ORN were shared, and how the network is doing."),
    ("themes", "Show off the wallet themes you build in Orien Wallet: Settings › Appearance › Build a theme."),
];

/// Each channel's introduction: the first message in it, saying what the
/// channel is for. #welcome has the welcome message instead, #announcements
/// is kept to announcements alone, and #general
/// and #help need none. Posted by hand with `orien-discord intros`; a
/// changed text here replaces the message already there.
pub const INTROS: [(&str, &str); 9] = [
    (
        "mining",
        "\
## ⛏️ Mining
Everything about mining ORN: your setup, your speed, your share of each hour's coins, and \"is my miner working?\"

- **New to mining?** Open Orien Wallet, press **Mine**, then **Start mining**. It needs about 2 GB of free memory.
- **Every hour**, the new ORN are shared among everyone who mined in that hour, by the work each one did.
- **Ask anything here.** Orien Bot answers questions in this channel, and other miners help too.",
    ),
    (
        "swaps-market",
        "\
## 🔁 Swaps and the Market
Find someone to swap with, and show what you offer in the Market.

- **Swaps:** trade ORN for SOL or Bitcoin directly with another person, from Orien Wallet on a computer. Either both of you get paid, or both get your coins back. Post here if you are looking for a buyer or a seller.
- **The Market:** share your listings, or ask for something you would like to pay for in ORN.
- **Do the deal in the app, never in private messages.** A swap in Orien Wallet looks after both sides. A promise in a private message looks after nobody.

⚠️ The team will never message you first. Never share your recovery phrase with anyone.",
    ),
    (
        "validators",
        "\
## 🛡️ Validators
For people who run a validator, or want to: staking, servers and the orien command line.

- **What a validator does:** it checks payments, and earns a share of the network's fees.
- **What it takes:** at least 10,000 ORN staked, and a server that stays on. The steps are at https://orien.network/try/
- **Ask here** about setting up, updating, and anything that looks wrong on your server.",
    ),
    (
        "bugs-and-ideas",
        "\
## 🐞 Bugs and ideas
Tell us what's broken, and what's missing.

- **Found a bug?** Say what you did, what you expected, and what happened instead. Add your Orien Wallet version and whether you are on Mac, Windows, Linux or iPhone. A screenshot helps.
- **Have an idea?** Describe what you would like to be able to do, and why.
- **Never post your recovery phrase or password**, even in a bug report. Nobody needs them to fix anything.",
    ),
    (
        "greetings",
        "\
## 👋 Greetings
Everyone who joins gets a hello here from Orien Bot, written just for them.

Say hi back, and make the new people feel at home. Longer chats go in {#general}.",
    ),
    (
        "x-posts",
        "\
## 📢 X posts
Orien's posts on X show up here as they go out, from https://x.com/oriennetwork

If you like one, a like or a repost on X helps more people find Orien.",
    ),
    (
        "youtube",
        "\
## 📺 YouTube
Orien's videos show up here as they come out, from https://www.youtube.com/@OrienNetwork

Watch them right here. A like or a subscribe on YouTube helps more people find Orien.",
    ),
    (
        "checkpoints",
        "\
## ⛏️ Checkpoints
Once an hour the network pauses for a moment, agrees on every balance, and shares out the hour's new ORN among everyone who mined. That moment is a checkpoint.

After each one, Orien Bot posts a summary here: how many miners were paid, how the ORN were shared, how much work was done, and how the network is doing. Every figure comes from the Orien Explorer at https://explorer.orien.network",
    ),
    (
        "themes",
        "\
## 🎨 Themes
Show off the wallet themes you build in Orien Wallet, and find ideas for your next one.

- **Build one:** open **Settings › Appearance** and press **Build a theme**. Give it a name, pick a dark or light base, one accent colour and a background picture, and the app makes the rest.
- **Share it here:** post a screenshot, and say which colour and picture you used so others can make one like it.
- **Keep your balance private:** you can hide it in Settings before you take the screenshot.",
    ),
];

/// A text with its channel links filled in. A channel the server does not
/// have stays as its plain name.
pub fn fill(template: &str, channel: impl Fn(&str) -> Option<u64>) -> String {
    let mut text = String::new();
    let mut rest = template;
    while let Some(start) = rest.find("{#") {
        let Some(len) = rest[start..].find('}') else { break };
        let name = &rest[start + 2..start + len];
        text.push_str(&rest[..start]);
        match channel(name) {
            Some(id) => text.push_str(&format!("<#{id}>")),
            None => text.push_str(&format!("#{name}")),
        }
        rest = &rest[start + len + 1..];
    }
    text.push_str(rest);
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_welcome_message_fits_in_one_discord_message() {
        // Channel links are about 22 characters each.
        let full = fill(WELCOME, |_| Some(1_234_567_890_123_456_789));
        assert!(full.chars().count() <= 2000, "{} characters", full.chars().count());
        assert!(!full.contains("{#"));
        assert!(full.contains("in <#1234567890123456789>."));
        assert!(fill(WELCOME, |_| None).contains("- #mining: setups"));
        assert!(fill(AFTER_HELLO, |_| Some(42)).contains("go in <#42>."));
        assert!(fit_hello(HELLO));
    }

    #[test]
    fn only_a_plain_hello_is_posted() {
        assert!(fit_hello("A miner named Luna? The night shift is covered. Welcome, {user}! 🌙"));
        for unfit in [
            "Welcome!",
            "Welcome {user}, and hello again {user}",
            "Welcome {user}, ORN will hit $10",
            "Welcome {user}, claim your airdrop at orien-gift.com",
            "Welcome {user}! See https://example.com",
            "Welcome {user}, ask @everyone",
            "Welcome {user}\nsecond line",
            "Welcome {user}, DM me your recovery phrase",
        ] {
            assert!(!fit_hello(unfit), "{unfit}");
        }
    }

    #[test]
    fn about_lines_fit_and_follow_the_wording_rules() {
        for (name, topic) in TOPICS {
            assert!(topic.chars().count() <= 1024, "{name}");
        }
        for (name, intro) in INTROS {
            assert!(fill(intro, |_| Some(1_234_567_890_123_456_789)).chars().count() <= 2000, "{name}");
            assert!(intro.starts_with("## "), "{name}");
        }
        let everything = format!("{WELCOME} {HELLO} {AFTER_HELLO} {} {}", INTROS.map(|(_, intro)| intro).join(" "), TOPICS.map(|(_, topic)| topic).join(" ")).to_lowercase();
        assert!(!everything.contains("money") && !everything.contains("digital currency"));
    }
}
