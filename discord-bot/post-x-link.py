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
import urllib.error
import urllib.request

TOKEN = os.path.expanduser("~/.config/orien/discord-bot.token")
CHANNEL = "x-posts"
LINK = re.compile(r"https://(?:x|twitter)\.com/oriennetwork/status/(\d{5,25})(?:\?.*)?", re.I)
API = "https://discord.com/api/v10"


def call(token: str, method: str, path: str, body=None):
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
    with urllib.request.urlopen(request, timeout=20) as response:
        return json.load(response)


def main() -> int:
    if len(sys.argv) != 2 or not (found := LINK.fullmatch(sys.argv[1].strip())):
        print("give one link like https://x.com/oriennetwork/status/1234567890")
        return 1
    link = f"https://x.com/oriennetwork/status/{found.group(1)}"
    try:
        token = open(TOKEN).read().strip()
        guilds = call(token, "GET", "/users/@me/guilds")
        if len(guilds) != 1:
            print(f"the bot is in {len(guilds)} servers; it should be in exactly one")
            return 1
        channels = call(token, "GET", f"/guilds/{guilds[0]['id']}/channels")
        channel = next((c for c in channels if c["type"] in (0, 5) and c["name"] == CHANNEL), None)
        if channel is None:
            print(f"the server has no #{CHANNEL} channel")
            return 1
        # The same post is never dropped twice.
        recent = call(token, "GET", f"/channels/{channel['id']}/messages?limit=100")
        if any(found.group(1) in message.get("content", "") for message in recent):
            print(f"already in #{CHANNEL}: {link}")
            return 0
        call(token, "POST", f"/channels/{channel['id']}/messages", {"content": link, "allowed_mentions": {"parse": []}})
    except urllib.error.HTTPError as error:
        print(f"Discord answered {error.code}; nothing posted")
        return 1
    except (OSError, ValueError) as error:
        print(f"could not post: {type(error).__name__}")
        return 1
    print(f"posted in #{CHANNEL}: {link}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
