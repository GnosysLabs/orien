#!/usr/bin/env bash
# Asks validator N, on this server, a simple question. Run every minute by
# orien-watchdog@N.timer. After ten minutes in a row without an answer it
# restarts the validator: a validator that runs but cannot answer helps no
# one, and the service only restarts one that has stopped.
#
# Ten minutes, not one: a validator can be busy for a few minutes at a close
# checking work shares, and restarting it then would only make it later.
#
#   orien-watchdog.sh N
set -u
N="$1"
UNIT="orien-validator@$N.service"
COUNT="/run/orien-watchdog-$N"
PATIENCE=10

# A validator that is stopped on purpose, or starting, is left alone.
if [ "$(systemctl is-active "$UNIT")" != active ]; then
    rm -f "$COUNT"
    exit 0
fi

ORIEN=/var/lib/orien/bin/orien
# Before orien 1.10.4 there is no ping to ask with.
if ! "$ORIEN" ping --help >/dev/null 2>&1; then
    exit 0
fi

if "$ORIEN" ping "127.0.0.1:710$N" --wait 20 >/dev/null 2>&1; then
    rm -f "$COUNT"
    exit 0
fi

missed=$(( $(cat "$COUNT" 2>/dev/null || echo 0) + 1 ))
echo "$missed" > "$COUNT"
echo "validator $N did not answer ($missed minute(s) in a row)"
if [ "$missed" -ge "$PATIENCE" ]; then
    echo "validator $N has not answered for $PATIENCE minutes; restarting it"
    rm -f "$COUNT"
    systemctl restart "$UNIT"
fi
