#!/usr/bin/env bash
# Read by HiveOS's agent every few seconds: sets $khs and $stats from what
# `orien mine` last wrote to its log ("… tries a second; N share(s) found,
# M taken by validators", every five seconds).

. /hive/miners/custom/orien/h-manifest.conf
orien_log="$CUSTOM_LOG_BASENAME.log"
orien_line=$(tail -n 300 "$orien_log" 2>/dev/null | grep "tries a second" | tail -n 1)
orien_age=$(( $(date +%s) - $(stat -c %Y "$orien_log" 2>/dev/null || echo 0) ))

orien_hs=0
orien_taken=0
if [[ -n $orien_line ]]; then
    # A line older than a minute means the miner stopped: no hashrate.
    if (( orien_age < 60 )); then
        if [[ $orien_line =~ ([0-9.]+)\ million\ tries ]]; then
            orien_hs=$(awk -v m="${BASH_REMATCH[1]}" 'BEGIN { printf "%.0f", m * 1000000 }')
        elif [[ $orien_line =~ ([0-9]+)\ tries ]]; then
            orien_hs=${BASH_REMATCH[1]}
        fi
    fi
    [[ $orien_line =~ ([0-9]+)\ taken ]] && orien_taken=${BASH_REMATCH[1]}
fi

orien_pid=$(pgrep -f "orien mine" | head -n 1)
orien_uptime=0
[[ -n $orien_pid ]] && orien_uptime=$(ps -o etimes= -p "$orien_pid" 2>/dev/null | tr -d ' ')
orien_version=$(/hive/miners/custom/orien/orien --version 2>/dev/null | awk '{ print $2 }')
orien_temp=$(cpu-temp 2>/dev/null | grep -o '^[0-9]*' | head -n 1)

khs=$(awk -v h="$orien_hs" 'BEGIN { printf "%.3f", h / 1000 }')
# Shares the validators took are "accepted"; one they didn't take this hour
# is not a rejection (it still counts), so nothing is shown as rejected.
stats=$(jq -nc \
    --argjson hs "$orien_hs" \
    --argjson uptime "${orien_uptime:-0}" \
    --argjson taken "$orien_taken" \
    --argjson temp "${orien_temp:-0}" \
    --arg ver "${orien_version:-$CUSTOM_VERSION}" \
    '{ hs: [$hs], hs_units: "hs", temp: [$temp], fan: [0], uptime: $uptime, ver: $ver, ar: [$taken, 0], algo: "randomx" }')
