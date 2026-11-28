#!/usr/bin/env bash
# Packs Orien for HiveOS: the scripts in hiveos/orien, the published Linux
# build of orien and the network's genesis file, as the custom-miner archive
# HiveOS installs from a flight sheet's Installation URL. It goes into
# web/public/downloads/hiveos/, which deploy/site.sh publishes, at
#
#   https://orien.network/downloads/hiveos/orien-VERSION.tar.gz
#
# The orien inside keeps itself current, so the package only needs packing
# again when its scripts change (raise CUSTOM_VERSION in h-manifest.conf).
#
#   deploy/hiveos.sh

set -euo pipefail
cd "$(dirname "$0")/.."

DOWNLOADS=web/public/downloads
VERSION=$(sed -n 's/^CUSTOM_VERSION=//p' hiveos/orien/h-manifest.conf)
ORIEN=$(cat "$DOWNLOADS/latest")
BUILD="orien-$ORIEN-linux-x86_64.tar.gz"

expected=$(grep " $BUILD\$" "$DOWNLOADS/SHA256SUMS" | cut -d ' ' -f 1)
actual=$(shasum -a 256 "$DOWNLOADS/$BUILD" | cut -d ' ' -f 1)
[ -n "$expected" ] && [ "$expected" = "$actual" ] || { echo "$BUILD does not match its published checksum" >&2; exit 1; }

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir "$work/orien"
cp hiveos/orien/h-manifest.conf hiveos/orien/h-config.sh hiveos/orien/h-run.sh hiveos/orien/h-stats.sh "$work/orien/"
cp web/public/network/genesis.json "$work/orien/genesis.json"
tar -xzf "$DOWNLOADS/$BUILD" -C "$work/orien" orien
chmod 755 "$work/orien" "$work/orien/orien" "$work/orien/"h-*.sh
chmod 644 "$work/orien/h-manifest.conf" "$work/orien/genesis.json"

mkdir -p "$DOWNLOADS/hiveos"
out="$DOWNLOADS/hiveos/orien-$VERSION.tar.gz"
COPYFILE_DISABLE=1 tar --no-xattrs --uid 0 --gid 0 --uname root --gname root -czf "$out" -C "$work" orien
echo "packed $out with orien $ORIEN"
tar -tvzf "$out"
