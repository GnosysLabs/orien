#!/usr/bin/env python3
"""Drops the link of one of Orien's X posts into the Discord server's
#x-posts channel, as the bot. Discord shows the post's own preview.

    python3 -I discord-bot/post-x-link.py https://x.com/oriennetwork/status/1234567890

It posts only links to posts by @oriennetwork, and nothing else: no text
of the caller's choosing. It uses the bot's token from
~/.config/orien/discord-bot.token and prints nothing secret. Exit 0 when
posted, 1 when not (the reason is printed), so the X posting tool can call
it after each post and carry on either way.
"""

import json
import os
import re
import sys
import time
import urllib.error
import urllib.request

TOKEN = os.path.expanduser("~/.config/orien/discord-bot.token")
CHANNEL = "x-posts"
REMEMBERED = os.path.expanduser("~/.config/orien/discord-x-posts.channel")
LINK = re.compile(r"https://(?:x|twitter)\.com/oriennetwork/status/(\d{5,25})(?:\?.*)?", re.I)
API = "https://discord.com/api/v10"


def call(token: str, method: str, path: str, body=None):
    """One request to Discord. When Discord says to slow down (429), it
    waits as long as Discord asks, up to four times, and tries again."""
    for attempt in range(5):
        request = urllib.request.Request(
            API + path,
            method=method,
            data=json.dumps(body).encode() if body is not None else None,
            headers={
                "Authorization": "Bot " + token,
                "User-Agent": "DiscordBot (https://orien.network, 0.1)",
                "Content-Type": "application/json",
            },
        )
        try:
            with urllib.request.urlopen(request, timeout=20) as response:
                return json.load(response)
        except urllib.error.HTTPError as error:
            if error.code != 429 or attempt == 4:
                raise
            try:
                wait = float(error.headers.get("Retry-After") or json.load(error).get("retry_after") or 2)
            except (ValueError, AttributeError):
                wait = 2.0
            time.sleep(min(max(wait, 0.5), 30) + 0.5)


def channel_id(token: str, fresh: bool = False):
    """The #x-posts channel's number. Looked up once and remembered in a
    file (it is not a secret), so a post costs Discord one question less."""
    if not fresh:
        try:
            remembered = open(REMEMBERED).read().strip()
            if remembered.isdigit():
                return remembered
        except OSError:
            pass
    guilds = call(token, "GET", "/users/@me/guilds")
    if len(guilds) != 1:
        raise ValueError(f"the bot is in {len(guilds)} servers; it should be in exactly one")
    channels = call(token, "GET", f"/guilds/{guilds[0]['id']}/channels")
    channel = next((c for c in channels if c["type"] in (0, 5) and c["name"] == CHANNEL), None)
    if channel is None:
        raise ValueError(f"the server has no #{CHANNEL} channel")
    try:
        with open(REMEMBERED, "w") as file:
            file.write(channel["id"] + "\n")
    except OSError:
        pass
    return channel["id"]


def drop(token: str, link: str, post_id: str, channel: str) -> bool:
    """Posts the link unless it is already there. True when it posted."""
    recent = call(token, "GET", f"/channels/{channel}/messages?limit=100")
    if any(post_id in message.get("content", "") for message in recent):
        return False
    call(token, "POST", f"/channels/{channel}/messages", {"content": link, "allowed_mentions": {"parse": []}})
    return True


def main() -> int:
    if len(sys.argv) != 2 or not (found := LINK.fullmatch(sys.argv[1].strip())):
        print("give one link like https://x.com/oriennetwork/status/1234567890")
        return 1
    link = f"https://x.com/oriennetwork/status/{found.group(1)}"
    try:
        token = open(TOKEN).read().strip()
        try:
            posted = drop(token, link, found.group(1), channel_id(token))
        except urllib.error.HTTPError as error:
            # The remembered channel is gone: look it up again, once.
            if error.code != 404:
                raise
            posted = drop(token, link, found.group(1), channel_id(token, fresh=True))
        if not posted:
            print(f"already in #{CHANNEL}: {link}")
            return 0
    except urllib.error.HTTPError as error:
        print(f"Discord answered {error.code}; nothing posted")
        return 1
    except ValueError as error:
        print(f"could not post: {error}")
        return 1
    except OSError as error:
        print(f"could not post: {type(error).__name__}")
        return 1
    print(f"posted in #{CHANNEL}: {link}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
