#!/usr/bin/env bash
# Builds the static Orien website in web/ and deploys it atomically to
# https://orien.network on cyphers-vps.
#
# Usage, from the repo root:
#   bash deploy/site.sh

set -euo pipefail
cd "$(dirname "$0")/.."

HOST=root@cyphers-vps
DOMAIN=orien.network
WEB_ROOT=/var/www/$DOMAIN
RELEASE=$(date +%Y%m%d-%H%M%S)
SSH="ssh -o BatchMode=yes -o ConnectTimeout=10"

echo "Building static site in web/"
(cd web && pnpm typecheck && pnpm build)

echo "Uploading web/out/ to $HOST:$WEB_ROOT/releases/$RELEASE"
$SSH "$HOST" "mkdir -p '$WEB_ROOT/releases/$RELEASE'"
rsync -az --delete web/out/ "$HOST:$WEB_ROOT/releases/$RELEASE/"

$SSH "$HOST" bash -s -- "$DOMAIN" "$WEB_ROOT" "$RELEASE" <<'REMOTE'
set -euo pipefail
DOMAIN=$1
WEB_ROOT=$2
RELEASE=$3

chmod -R a+rX "$WEB_ROOT/releases/$RELEASE"
# Wallet app downloads and updates live outside the releases, so they
# survive every deploy; apps/wallet/release.sh fills the folder.
mkdir -p "$WEB_ROOT/wallet"
ln -sfn "$WEB_ROOT/wallet" "$WEB_ROOT/releases/$RELEASE/wallet"
ln -sfn "$WEB_ROOT/releases/$RELEASE" "$WEB_ROOT/current"

# Keep the three newest releases.
ls -1dt "$WEB_ROOT"/releases/* | tail -n +4 | xargs -r rm -rf
REMOTE

# First-time nginx + Let's Encrypt setup if the site is not enabled yet.
if ! $SSH "$HOST" "[ -e /etc/nginx/sites-enabled/$DOMAIN ]"; then
    echo "Installing nginx site and provisioning TLS certificate for $DOMAIN"
    scp -q deploy/nginx-$DOMAIN.conf "$HOST:/etc/nginx/sites-available/$DOMAIN"
    $SSH "$HOST" "ln -sfn /etc/nginx/sites-available/$DOMAIN /etc/nginx/sites-enabled/$DOMAIN \
        && nginx -t && systemctl reload nginx \
        && certbot --nginx -d $DOMAIN -d www.$DOMAIN --non-interactive --agree-tos --redirect"
fi

echo "Live at https://$DOMAIN ($RELEASE)"
