#!/usr/bin/env bash
# Runs `orien mine` with the flight sheet's settings. orien keeps itself on
# the newest release signed with the Orien release key, replacing the copy
# in this folder, so the rig never mines on rules that have changed.

cd "$(dirname "$0")"
. ./h-manifest.conf
mkdir -p "$(dirname "$CUSTOM_LOG_BASENAME")"

args=$(cat "$CUSTOM_CONFIG_FILENAME" 2>/dev/null)
if [[ -z $args || $args == ERROR:* ]]; then
    echo "${args:-ERROR: no settings; check the flight sheet}" | tee --append "$CUSTOM_LOG_BASENAME.log"
    sleep 30
    exit 1
fi

chmod +x ./orien
# shellcheck disable=SC2086
./orien mine $args --genesis "$PWD/genesis.json" 2>&1 | tee --append "$CUSTOM_LOG_BASENAME.log"
