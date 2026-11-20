#!/usr/bin/env bash
# Builds the Chrome extension in apps/wallet/chrome/dist (and a zip for the
# Chrome Web Store) from the Safari extension's window and pictures, with
# Chrome's own manifest, bridge and background.
set -euo pipefail
cd "$(dirname "$0")"
rm -rf dist && mkdir dist
for f in ../safari/Resources/*; do
  case "$(basename "$f")" in manifest.json|bridge.js|background.js) ;; *) cp "$f" dist/ ;; esac
done
cp manifest.json bridge.js background.js chrome-toolbar-*.png dist/
# The store gives its own ID and refuses "key": its zip leaves it out.
rm -f orien-wallet-chrome.zip
(cd dist && python3 -c "import json; m=json.load(open('manifest.json')); m.pop('key',None); open('/tmp/ow-store-manifest.json','w').write(json.dumps(m, indent=2))")
mkdir -p /tmp/ow-store && rm -rf /tmp/ow-store/* && cp dist/* /tmp/ow-store/ && cp /tmp/ow-store-manifest.json /tmp/ow-store/manifest.json
(cd /tmp/ow-store && zip -q -r "$OLDPWD/orien-wallet-chrome.zip" .)
rm -rf /tmp/ow-store /tmp/ow-store-manifest.json
echo "Chrome extension: $(pwd)/dist (load unpacked), $(pwd)/orien-wallet-chrome.zip (store)"
