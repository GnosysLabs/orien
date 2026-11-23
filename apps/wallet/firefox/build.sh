#!/usr/bin/env bash
# Builds the Firefox extension in apps/wallet/firefox/dist (and a zip for
# Firefox Add-ons) from the Safari extension: the same window, pictures,
# bridge and background (both browsers reach Orien Wallet through native
# messaging), with Firefox's own manifest and Chrome's toolbar pictures.
set -euo pipefail
cd "$(dirname "$0")"
rm -rf dist && mkdir dist
for f in ../safari/Resources/*; do
  case "$(basename "$f")" in manifest.json|toolbar-*.png) ;; *) cp "$f" dist/ ;; esac
done
cp manifest.json ../chrome/chrome-toolbar-*.png dist/
rm -f orien-wallet-firefox.zip
(cd dist && zip -q -r ../orien-wallet-firefox.zip .)
echo "Firefox extension: $(pwd)/dist (load as a temporary add-on), $(pwd)/orien-wallet-firefox.zip (Firefox Add-ons)"
