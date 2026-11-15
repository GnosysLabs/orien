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

# The orien downloads are not in git: deploy/release.sh builds them into
# web/public/downloads. Deploying from a checkout without them would take
# them off the live site and break the installer.
if [ ! -f web/public/downloads/latest ]; then
    echo "web/public/downloads is missing here, so this deploy would break the orien installer." >&2
    echo "Deploy from the checkout that has it, or copy it in first." >&2
    exit 1
fi

echo "Building static site in web/"
(cd web && pnpm typecheck && pnpm build)

# The marketplace's pages need its server, which is not running yet. Until it
# is, they stay off the live site. MARKETPLACE=1 publishes them.
if [ "${MARKETPLACE:-0}" != 1 ]; then
    rm -rf web/out/marketplace
    find web/out -name '*marketplace*' -prune -exec rm -rf {} +
fi

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
