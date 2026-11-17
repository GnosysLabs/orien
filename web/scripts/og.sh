#!/usr/bin/env bash
# Draws public/og.png, the image shown when orien.network is shared, from
# scripts/og.html and the latest build in out/ (run deploy/site.sh first).
# The background is brand/source/nebula.jpg.
set -euo pipefail
cd "$(dirname "$0")/.."
out=out
css=$(grep -o '<link rel="stylesheet" href="[^"]*"' "$out/index.html" | head -1 | sed 's/.*href="//; s/"$//')
classes=$(grep -o '<html[^>]*class="[^"]*"' "$out/index.html" | head -1 | sed 's/.*class="//; s/"$//')
sed -e "s|{{CSS}}|$css|" -e "s|{{HTML_CLASS}}|$classes|" scripts/og.html > "$out/_og.html"
cp ../brand/source/nebula.jpg "$out/_og-nebula.jpg"
port=8799
python3 -m http.server "$port" --directory "$out" >/dev/null 2>&1 &
server=$!
trap 'kill $server 2>/dev/null; rm -f "$out/_og.html" "$out/_og-nebula.jpg"' EXIT
sleep 1
chrome="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
profile=$(mktemp -d)
"$chrome" --headless=new --disable-gpu --hide-scrollbars --user-data-dir="$profile" --window-size=1200,630 \
  --virtual-time-budget=4000 --screenshot="$PWD/public/og.png" "http://127.0.0.1:$port/_og.html" 2>/dev/null
rm -rf "$profile"
echo "public/og.png drawn"
