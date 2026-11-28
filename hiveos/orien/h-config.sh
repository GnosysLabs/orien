#!/usr/bin/env bash
# Called by HiveOS each time the miner starts, with the flight sheet's
# settings in scope. Writes the arguments for `orien mine` to orien.conf:
#   Wallet and worker template: %WAL%  (the ORN address to be paid)
#   Extra config arguments:     --threads 8, or --cpu 75 (default: every core)

orien_dir=/hive/miners/custom/orien
. "$orien_dir/h-manifest.conf"

# The address, as the template made it: anything after a dot or a space
# (a worker name, say) is left off.
orien_address=$(printf '%s' "$CUSTOM_TEMPLATE" | tr -d '\r' | sed 's/[. ].*//' | tr 'A-F' 'a-f')
orien_extra=$(printf '%s' "$CUSTOM_USER_CONFIG" | tr -d '\r' | tr '\n' ' ')

if ! [[ $orien_address =~ ^[0-9a-f]{64}$ ]]; then
    echo "ERROR: the flight sheet's wallet must be an ORN address (64 letters and digits, 0-9 and a-f), not \"$CUSTOM_TEMPLATE\"" > "$CUSTOM_CONFIG_FILENAME"
elif [[ $orien_extra =~ --threads|--cpu ]]; then
    echo "$orien_address $orien_extra" > "$CUSTOM_CONFIG_FILENAME"
else
    # A rig is for mining: every core, unless the flight sheet says otherwise.
    echo "$orien_address --cpu 100 $orien_extra" > "$CUSTOM_CONFIG_FILENAME"
fi
