#!/usr/bin/env bash
# Builds the orien program for every supported computer and packs each build
# with a checksum, into web/public/downloads/, which the website serves.
# Publishing the website (deploy/site.sh) publishes them.
#
#   deploy/release.sh
#
# Needs, on a Mac: the Rust targets aarch64-apple-darwin,
# x86_64-unknown-linux-gnu, aarch64-unknown-linux-gnu and x86_64-pc-windows-gnu,
# and cargo-zigbuild with zig for the Linux and Windows builds. Macs with
# Intel chips are not supported, by the founder's choice.

set -euo pipefail
cd "$(dirname "$0")/.."

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
OUT="${ORIEN_RELEASE_OUT:-web/public/downloads}"
# The Orien release key, which signs the checksums so that `orien update`
# installs only genuine builds. Keep it secret and backed up offline.
KEY="${ORIEN_RELEASE_KEY:-$HOME/.config/orien/release.key}"
[ -f "$KEY" ] || { echo "no release key at $KEY"; exit 1; }

# name | Rust target | how to build | folder under target/
BUILDS="
macos-arm64  aarch64-apple-darwin           cargo     aarch64-apple-darwin
linux-x86_64 x86_64-unknown-linux-gnu.2.31  zigbuild  x86_64-unknown-linux-gnu
linux-arm64  aarch64-unknown-linux-gnu.2.31 zigbuild  aarch64-unknown-linux-gnu
windows-x86_64 x86_64-pc-windows-gnu        zigbuild  x86_64-pc-windows-gnu
"

rm -rf "$OUT"
mkdir -p "$OUT"
while read -r name target builder folder; do
    [ -n "$name" ] || continue
    echo "building orien $VERSION for $name"
    if [ "$builder" = zigbuild ]; then
        # RandomX is C++: on Windows its runtime comes from zig's libc++.
        CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS="-C link-arg=-lc++" \
            cargo zigbuild --quiet --release --target "$target" -p orien-cli
    else
        cargo build --quiet --release --target "$target" -p orien-cli
    fi
    if [ -f "target/$folder/release/orien.exe" ] && [[ "$name" == windows-* ]]; then
        # Windows users expect a zip.
        (cd "target/$folder/release" && rm -f "$OLDPWD/$OUT/orien-$VERSION-$name.zip" && zip -q "$OLDPWD/$OUT/orien-$VERSION-$name.zip" orien.exe)
    else
        # Without the Mac's own file tags, which Linux's tar warns about.
        COPYFILE_DISABLE=1 tar --no-xattrs -czf "$OUT/orien-$VERSION-$name.tar.gz" -C "target/$folder/release" orien
    fi
done <<< "$BUILDS"

# The installer reads which version is current, and checks each download
# against these checksums.
(cd "$OUT" && shasum -a 256 orien-* > SHA256SUMS)
cargo run --quiet --release -p orien-cli -- release sign --key "$KEY" "$OUT/SHA256SUMS"
echo "$VERSION" > "$OUT/latest"

echo
echo "in $OUT:"
(cd "$OUT" && ls -l | awk 'NR > 1 { printf "  %-34s %6.1f MB\n", $9, $5 / 1048576 }')
