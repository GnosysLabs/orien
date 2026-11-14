#!/usr/bin/env bash
# Builds, signs and publishes a new version of the Orien wallet app.
#
# Usage, from anywhere:
#   bash apps/wallet/release.sh 0.2.0                 build and publish
#   bash apps/wallet/release.sh 0.2.0 --minimum 0.2.0 also tell older copies
#                                                      they no longer work
#   bash apps/wallet/release.sh 0.2.0 --no-publish    build only, to check
#
# What it does:
#   1. Sets the version in tauri.conf.json and Cargo.toml.
#   2. Builds Orien.app and Orien.dmg for every Mac (Apple chips and Intel),
#      for the live network.
#   3. Signs the update with the Orien update key, which never leaves this
#      Mac: ~/.orien-release/wallet-update.key, or $ORIEN_UPDATE_KEY. It asks
#      for the key's password.
#   4. If Apple signing is set up (see below), signs the app as Orien's and
#      has Apple check it, so Macs open it without a warning.
#   5. Uploads to https://orien.network/wallet/<version>/, then replaces
#      https://orien.network/wallet/latest.json, which is what copies of the
#      app ask. Until that last step nobody is offered the new version.
#
# Apple signing, once the developer account is active. Put these in your
# shell (never in this repository):
#   export APPLE_SIGNING_IDENTITY="Developer ID Application: <name> (<team id>)"
#   export APPLE_ID=<your Apple ID email>
#   export APPLE_PASSWORD=<an app-specific password from appleid.apple.com>
#   export APPLE_TEAM_ID=<team id>

set -euo pipefail
cd "$(dirname "$0")"
APP=$(pwd)
ROOT=$(cd ../.. && pwd)

VERSION=${1:?"say which version, such as: bash apps/wallet/release.sh 0.2.0"}
shift
MINIMUM=""
PUBLISH=1
while [ $# -gt 0 ]; do
    case $1 in
        --minimum) MINIMUM=${2:?"--minimum needs a version"}; shift 2 ;;
        --no-publish) PUBLISH=0; shift ;;
        *) echo "unknown option: $1" >&2; exit 1 ;;
    esac
done
[[ $VERSION =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "a version looks like 0.2.0" >&2; exit 1; }

HOST=root@cyphers-vps
WEB_ROOT=/var/www/orien.network
BASE_URL=https://orien.network/wallet
SSH="ssh -o BatchMode=yes -o ConnectTimeout=10"
TARGET=universal-apple-darwin

KEY=${ORIEN_UPDATE_KEY:-$HOME/.orien-release/wallet-update.key}
[ -f "$KEY" ] || { echo "no update key at $KEY" >&2; exit 1; }
if [ $PUBLISH = 1 ] && [ -n "$(git -C "$ROOT" status --porcelain -- apps/wallet)" ]; then
    echo "apps/wallet has uncommitted changes; commit them first, so every release is in git" >&2
    exit 1
fi

echo "Setting the version to $VERSION"
python3 - "$VERSION" <<'PY'
import json, re, sys
version = sys.argv[1]
config = json.load(open("tauri.conf.json"))
config["version"] = version
with open("tauri.conf.json", "w") as f:
    json.dump(config, f, indent=2)
    f.write("\n")
cargo = open("Cargo.toml").read()
cargo = re.sub(r'(?m)^version = "[^"]*"', f'version = "{version}"', cargo, count=1)
open("Cargo.toml", "w").write(cargo)
PY

if [ -z "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}" ]; then
    read -r -s -p "Password for the update key: " TAURI_SIGNING_PRIVATE_KEY_PASSWORD
    echo
fi
export TAURI_SIGNING_PRIVATE_KEY=$KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD
if [ -z "${APPLE_SIGNING_IDENTITY:-}" ]; then
    echo "Note: Apple signing is not set up, so Macs will warn before opening the downloaded app."
fi

echo "Building Orien $VERSION for every Mac"
rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null
# Built for the live network: the genesis file in web/public/network.
env -u ORIEN_GENESIS cargo tauri build --target $TARGET

BUNDLE=$APP/target/$TARGET/release/bundle
OUT=$APP/target/release-$VERSION
rm -rf "$OUT" && mkdir -p "$OUT"
cp "$BUNDLE/macos/Orien.app.tar.gz" "$OUT/Orien.app.tar.gz"
cp "$BUNDLE"/dmg/Orien_"$VERSION"_*.dmg "$OUT/Orien.dmg"
SIGNATURE=$(cat "$BUNDLE/macos/Orien.app.tar.gz.sig")
(cd "$OUT" && shasum -a 256 Orien.dmg Orien.app.tar.gz > SHA256SUMS)

python3 - "$VERSION" "$MINIMUM" "$SIGNATURE" "$BASE_URL" "$OUT/latest.json" <<'PY'
import json, sys, datetime
version, minimum, signature, base, out = sys.argv[1:]
url = f"{base}/{version}/Orien.app.tar.gz"
platform = {"signature": signature, "url": url}
latest = {
    "version": version,
    "notes": f"Orien {version}",
    "pub_date": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    # One app runs on both kinds of Mac.
    "platforms": {"darwin-aarch64": platform, "darwin-x86_64": platform},
}
if minimum:
    latest["minimum"] = minimum
json.dump(latest, open(out, "w"), indent=2)
PY

echo "Built in $OUT:"
cat "$OUT/SHA256SUMS"
if [ $PUBLISH = 0 ]; then
    echo "Not published (--no-publish)."
    exit 0
fi

echo "Uploading to $HOST:$WEB_ROOT/wallet/$VERSION"
$SSH "$HOST" "mkdir -p '$WEB_ROOT/wallet/$VERSION'"
rsync -az "$OUT/Orien.app.tar.gz" "$OUT/Orien.dmg" "$OUT/SHA256SUMS" "$HOST:$WEB_ROOT/wallet/$VERSION/"
rsync -az "$OUT/latest.json" "$HOST:$WEB_ROOT/wallet/latest.json.new"
$SSH "$HOST" bash -s -- "$WEB_ROOT" "$VERSION" <<'REMOTE'
set -euo pipefail
WEB_ROOT=$1
VERSION=$2
chmod -R a+rX "$WEB_ROOT/wallet"
# The download link on the website always points at the newest.
ln -sfn "$VERSION/Orien.dmg" "$WEB_ROOT/wallet/Orien.dmg"
# The website is served from its newest release; link the wallet folder in.
ln -sfn "$WEB_ROOT/wallet" "$WEB_ROOT/current/wallet"
# Last, and in one step: copies of the app now see the new version.
mv "$WEB_ROOT/wallet/latest.json.new" "$WEB_ROOT/wallet/latest.json"
REMOTE

git -C "$ROOT" add apps/wallet/tauri.conf.json apps/wallet/Cargo.toml apps/wallet/Cargo.lock
git -C "$ROOT" commit -q -m "Wallet app $VERSION" || true
echo "Published Orien $VERSION: $BASE_URL/latest.json"
