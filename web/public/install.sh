#!/bin/sh
# Installs orien, the program for the Orien cryptocurrency, on a Mac or Linux
# computer:
#
#   curl -fsSL https://orien.network/install.sh | sh
#
# It downloads the build for this computer, checks it against the published
# checksum, and installs it into ~/.orien/bin. Nothing outside ~/.orien is
# touched except one line added to your shell's startup file, so that
# `orien` can be found; set ORIEN_NO_MODIFY_PATH=1 to skip that.
#
# ORIEN_HOME changes where it installs; ORIEN_BASE where it downloads from.

set -eu

BASE="${ORIEN_BASE:-https://orien.network}"
HOME_DIR="${ORIEN_HOME:-$HOME/.orien}"

say() { printf '%s\n' "$*"; }
fail() { printf 'orien installer: %s\n' "$*" >&2; exit 1; }

command -v curl > /dev/null || fail "curl is needed"
command -v tar > /dev/null || fail "tar is needed"

case "$(uname -s)" in
    Darwin) os=macos ;;
    Linux) os=linux ;;
    *) fail "this installer is for Mac and Linux; on Windows, run in PowerShell: irm $BASE/install.ps1 | iex" ;;
esac
case "$(uname -m)" in
    arm64 | aarch64) arch=arm64 ;;
    x86_64 | amd64) arch=x86_64 ;;
    *) fail "there is no orien build for a $(uname -m) processor" ;;
esac
[ "$os-$arch" = macos-x86_64 ] && fail "orien runs on Macs with Apple chips (M1 or later), not Intel Macs"

version="$(curl -fsSL "$BASE/downloads/latest")" || fail "could not reach $BASE"
file="orien-$version-$os-$arch.tar.gz"
say "installing orien $version for $os ($arch)"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
curl -fsSL "$BASE/downloads/$file" -o "$work/$file" || fail "could not download $file"
curl -fsSL "$BASE/downloads/SHA256SUMS" -o "$work/SHA256SUMS" || fail "could not download the checksums"

# A damaged or altered download is refused.
expected="$(grep " $file\$" "$work/SHA256SUMS" | cut -d ' ' -f 1)"
[ -n "$expected" ] || fail "no published checksum for $file"
if command -v sha256sum > /dev/null; then
    actual="$(sha256sum "$work/$file" | cut -d ' ' -f 1)"
else
    actual="$(shasum -a 256 "$work/$file" | cut -d ' ' -f 1)"
fi
[ "$actual" = "$expected" ] || fail "the download does not match its published checksum; nothing was installed"

mkdir -p "$HOME_DIR/bin"
tar -xzf "$work/$file" -C "$work"
mv -f "$work/orien" "$HOME_DIR/bin/orien"
chmod 755 "$HOME_DIR/bin/orien"

# The network's starting file, when one is published.
if curl -fsSL "$BASE/network/genesis.json" -o "$work/genesis.json" 2> /dev/null; then
    mv -f "$work/genesis.json" "$HOME_DIR/genesis.json"
fi

# Make `orien` findable from new terminals.
path_line="export PATH=\"$HOME_DIR/bin:\$PATH\""
case ":$PATH:" in
    *":$HOME_DIR/bin:"*) on_path=yes ;;
    *) on_path=no ;;
esac
if [ "$on_path" = no ] && [ "${ORIEN_NO_MODIFY_PATH:-0}" != 1 ]; then
    case "${SHELL:-}" in
        */zsh) profile="$HOME/.zshrc" ;;
        */bash) [ "$os" = macos ] && profile="$HOME/.bash_profile" || profile="$HOME/.bashrc" ;;
        *) profile="$HOME/.profile" ;;
    esac
    if ! grep -qs "$HOME_DIR/bin" "$profile"; then
        printf '\n# orien\n%s\n' "$path_line" >> "$profile"
        say "added orien to your PATH in $profile"
    fi
fi

say ""
say "installed: $("$HOME_DIR/bin/orien" --version)"
if [ "$on_path" = no ]; then
    say "open a new terminal, or run this once in this one:"
    say "  $path_line"
fi
say "then try: orien --help"
