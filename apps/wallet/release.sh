#!/usr/bin/env bash
# Builds, signs and publishes a new version of Orien Wallet, the desktop app.
#
# Usage, from anywhere:
#   bash apps/wallet/release.sh 0.2.0                 build and publish
#   bash apps/wallet/release.sh 0.2.0 --minimum 0.2.0 also tell older copies
#                                                      they no longer work
#   bash apps/wallet/release.sh 0.2.0 --no-publish    build only, to check
#
# What it does:
#   1. Sets the version in tauri.conf.json and Cargo.toml.
#   2. Builds Orien Wallet.app and its .dmg for Macs with Apple chips, for the
#      live network. Intel Macs are not supported.
#   3. Signs the update with the Orien update key, which never leaves this
#      Mac: ~/.orien-release/wallet-update.key, or $ORIEN_UPDATE_KEY. It asks
#      for the key's password.
#   4. If Apple signing is set up (see below), signs the app as Orien's and
#      has Apple check it, so Macs open it without a warning.
#   5. Uploads to https://orien.network/wallet/<version>/, then replaces
#      https://orien.network/wallet/latest.json, which is what copies of the
#      app ask. Until that last step nobody is offered the new version.
#
# Apple signing uses what is already on this Mac, nothing in this repository:
#   - the "Developer ID Application" certificate in the login keychain,
#     found automatically (or name it in APPLE_SIGNING_IDENTITY);
#   - the notary login saved in the keychain with
#       xcrun notarytool store-credentials AC_NOTARY --team-id <team id>
#     (another name in APPLE_KEYCHAIN_PROFILE).
# The script signs the app, has Apple notarize the .dmg, staples Apple's
# ticket to both, and only then packs and signs the update, so updates carry
# the notarized app too.

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
TARGET=aarch64-apple-darwin

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
    APPLE_SIGNING_IDENTITY=$(security find-identity -v -p codesigning | sed -n 's/.*"\(Developer ID Application: .*\)"/\1/p' | head -1)
fi
NOTARY=${APPLE_KEYCHAIN_PROFILE:-AC_NOTARY}
if [ -n "$APPLE_SIGNING_IDENTITY" ]; then
    echo "Signing as: $APPLE_SIGNING_IDENTITY"
    export APPLE_SIGNING_IDENTITY
    # Fails early, before the build, if Apple's notary service would refuse.
    xcrun notarytool history --keychain-profile "$NOTARY" >/dev/null || {
        echo "Apple's notary service refused the '$NOTARY' login. Check the agreements at developer.apple.com/account." >&2
        exit 1
    }
else
    echo "Note: no Developer ID certificate on this Mac, so Macs will warn before opening the downloaded app."
fi

echo "Building Orien $VERSION for Macs with Apple chips"
rustup target add $TARGET >/dev/null
# Built for the live network: the genesis file in web/public/network.
# Tauri signs the app here; notarizing is done below, with the keychain
# login, so Tauri is not given Apple credentials of its own.
env -u ORIEN_GENESIS -u APPLE_ID -u APPLE_PASSWORD -u APPLE_API_KEY -u APPLE_API_ISSUER \
    cargo tauri build --target $TARGET

BUNDLE=$APP/target/$TARGET/release/bundle
OUT=$APP/target/release-$VERSION
rm -rf "$OUT" && mkdir -p "$OUT"
cp "$BUNDLE"/dmg/"Orien Wallet_$VERSION"_*.dmg "$OUT/Orien-Wallet.dmg"
if [ -n "$APPLE_SIGNING_IDENTITY" ]; then
    echo "Sending to Apple for notarization (usually a few minutes)"
    xcrun notarytool submit "$OUT/Orien-Wallet.dmg" --keychain-profile "$NOTARY" --wait | tee "$OUT/notary.log"
    grep -q "status: Accepted" "$OUT/notary.log" || { echo "Apple did not accept it; see $OUT/notary.log" >&2; exit 1; }
    xcrun stapler staple "$OUT/Orien-Wallet.dmg"
    xcrun stapler staple "$BUNDLE/macos/Orien Wallet.app"
    spctl -a -vv "$BUNDLE/macos/Orien Wallet.app"
fi
# The update: the (now notarized) app, packed and signed with the update key.
COPYFILE_DISABLE=1 tar -czf "$OUT/Orien-Wallet.app.tar.gz" -C "$BUNDLE/macos" "Orien Wallet.app"
# The password comes from the environment, not the command line.
env -u TAURI_SIGNING_PRIVATE_KEY cargo tauri signer sign -f "$KEY" "$OUT/Orien-Wallet.app.tar.gz" >/dev/null
SIGNATURE=$(cat "$OUT/Orien-Wallet.app.tar.gz.sig")
(cd "$OUT" && shasum -a 256 Orien-Wallet.dmg Orien-Wallet.app.tar.gz > SHA256SUMS)

python3 - "$VERSION" "$MINIMUM" "$SIGNATURE" "$BASE_URL" "$OUT/latest.json" <<'PY'
import json, sys, datetime
version, minimum, signature, base, out = sys.argv[1:]
url = f"{base}/{version}/Orien-Wallet.app.tar.gz"
platform = {"signature": signature, "url": url}
latest = {
    "version": version,
    "notes": f"Orien {version}",
    "pub_date": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    # Macs with Apple chips only.
    "platforms": {"darwin-aarch64": platform},
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
rsync -az "$OUT/Orien-Wallet.app.tar.gz" "$OUT/Orien-Wallet.dmg" "$OUT/SHA256SUMS" "$HOST:$WEB_ROOT/wallet/$VERSION/"
rsync -az "$OUT/latest.json" "$HOST:$WEB_ROOT/wallet/latest.json.new"
$SSH "$HOST" bash -s -- "$WEB_ROOT" "$VERSION" <<'REMOTE'
set -euo pipefail
WEB_ROOT=$1
VERSION=$2
chmod -R a+rX "$WEB_ROOT/wallet"
# The download link on the website always points at the newest.
ln -sfn "$VERSION/Orien-Wallet.dmg" "$WEB_ROOT/wallet/Orien-Wallet.dmg"
# The website is served from its newest release; link the wallet folder in.
ln -sfn "$WEB_ROOT/wallet" "$WEB_ROOT/current/wallet"
# Last, and in one step: copies of the app now see the new version.
mv "$WEB_ROOT/wallet/latest.json.new" "$WEB_ROOT/wallet/latest.json"
REMOTE

git -C "$ROOT" add apps/wallet/tauri.conf.json apps/wallet/Cargo.toml apps/wallet/Cargo.lock
git -C "$ROOT" commit -q -m "Wallet app $VERSION" || true
echo "Published Orien $VERSION: $BASE_URL/latest.json"
