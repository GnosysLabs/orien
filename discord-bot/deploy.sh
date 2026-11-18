#!/bin/bash
# Puts the Discord bot on the project's server, or brings it up to date.
# Run from the repository's top folder:
#
#     bash discord-bot/deploy.sh
#
# It builds the bot for Linux here, gathers what the bot knows, and copies
# to the server: the program, its service file, the two keys and
# knowledge.md. Nothing else on the server is touched.
set -euo pipefail

HOST=${HOST:-root@cyphers-vps}
TARGET=x86_64-unknown-linux-gnu
TOKEN=~/.config/orien/discord-bot.token
KEY=~/.config/orien/openrouter.key

cd "$(dirname "$0")/.."
[ -s "$TOKEN" ] && [ -s "$KEY" ] || { echo "missing $TOKEN or $KEY"; exit 1; }

python3 discord-bot/build-knowledge.py
(cd discord-bot && cargo zigbuild --quiet --release --target $TARGET.2.31)
PROGRAM=discord-bot/target/$TARGET/release/orien-discord

ssh "$HOST" 'id orien-discord >/dev/null 2>&1 || useradd --system --no-create-home --shell /usr/sbin/nologin orien-discord
install -d -o orien-discord -g orien-discord -m 700 /etc/orien-discord'
scp -q "$PROGRAM" "$HOST:/usr/local/bin/orien-discord.new"
scp -q discord-bot/orien-discord.service "$HOST:/etc/systemd/system/orien-discord.service"
scp -q "$TOKEN" "$KEY" discord-bot/knowledge.md web/public/network/genesis.json "$HOST:/etc/orien-discord/"
ssh "$HOST" 'chown orien-discord:orien-discord /etc/orien-discord/* && chmod 600 /etc/orien-discord/*
chmod 755 /usr/local/bin/orien-discord.new && mv /usr/local/bin/orien-discord.new /usr/local/bin/orien-discord
systemctl daemon-reload && systemctl enable --quiet orien-discord && systemctl restart orien-discord
sleep 5 && systemctl is-active orien-discord && journalctl -u orien-discord -n 5 --no-pager -o cat'
