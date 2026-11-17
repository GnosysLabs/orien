#!/usr/bin/env python3
"""Gathers what the Discord bot is allowed to know into knowledge.md.

Everything comes from the project's own material: the website's pages, the
paper, the changelog, docs/epochs.md and the orien program's own help. Run it
from the repository's top folder after any of those change, then restart the
bot:

    python3 discord-bot/build-knowledge.py
"""

import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = Path(__file__).resolve().parent / "knowledge.md"

# (heading, file, the page's address on the site)
PAGES = [
    ("Home page", "web/src/app/page.tsx", "https://orien.network/"),
    ("Get started (download the wallet, mine, send, run a validator)", "web/src/app/try/page.tsx", "https://orien.network/try/"),
    ("How it works", "web/src/app/how-it-works/page.tsx", "https://orien.network/how-it-works/"),
    ("News: mining with RandomX", "web/src/app/news/randomx/page.tsx", "https://orien.network/news/randomx/"),
    ("Contact", "web/src/app/contact/page.tsx", "https://orien.network/contact/"),
    ("The Orien paper", "web/src/app/paper/page.tsx", "https://orien.network/paper/"),
]


def page_text(source: str) -> str:
    """A page's words, without its layout code."""
    # Imports and styling carry no facts.
    source = re.sub(r"^import .*$", "", source, flags=re.M)
    source = re.sub(r"/\*.*?\*/", "", source, flags=re.S)
    source = re.sub(r"\s(className|aria-hidden|aria-label|strokeWidth|viewBox|d|fill|stroke)=\"[^\"]*\"", "", source)
    source = re.sub(r"\s(className|strokeWidth)=\{[^{}]*\}", "", source)
    source = source.replace("{' '}", " ").replace('{" "}', " ")
    # Arrows would be mistaken for the end of a tag.
    source = source.replace("=>", "→")
    # Headings keep their weight; other tags go.
    source = re.sub(r"<h[1-4][^>]*>", "\n\n### ", source)
    source = re.sub(r"<(PaperSection|Step)\b[^>]*title=\"([^\"]*)\"[^>]*>", r"\n\n### \2\n", source)
    source = re.sub(r"<li[^>]*>", "\n- ", source)
    source = re.sub(r"</(p|h[1-4]|li|section|header|div|tr|ul|ol)>", "\n", source)
    source = re.sub(r"</?[A-Za-z][^<>]*>", "", source)
    for entity, char in (("&nbsp;", " "), ("&amp;", "&"), ("&lt;", "<"), ("&gt;", ">"), ("&apos;", "'"), ("&quot;", '"')):
        source = source.replace(entity, char)
    lines = [re.sub(r"[ \t]+", " ", line).strip() for line in source.splitlines()]
    # Leftover code lines: brackets and keywords only.
    lines = [line for line in lines if not re.fullmatch(r"[\s(){}\[\];,.→?:]*|return \(|\)\s*}?", line)]
    text = "\n".join(lines)
    return re.sub(r"\n{3,}", "\n\n", text).strip()


def changelog_text(source: str) -> str:
    """The changelog's entries, without the entries not released yet."""
    start = source.index("export const CHANGELOG")
    body = source[start:]
    entries = re.split(r"\n  \{\n", body)[1:]
    kept = []
    for entry in entries:
        if re.search(r"unreleased:\s*true", entry):
            continue
        entry = re.sub(r"\n  \},?\s*(\]\s*)?$", "", entry.rstrip())
        kept.append(re.sub(r"^    ", "", entry, flags=re.M))
    return "\n\n".join(kept)


def orien_help() -> str:
    """What `orien --help` and each command's help say."""
    # The installed, released orien first: its help is what people have.
    for program in (Path.home() / ".orien/bin/orien", ROOT / "target/release/orien"):
        if program.exists():
            break
    else:
        print("no orien program found; leaving its help out", file=sys.stderr)
        return ""

    def run(*args: str) -> str:
        return subprocess.run([str(program), *args], capture_output=True, text=True, timeout=20).stdout.strip()

    top = run("--help")
    parts = [f"$ orien --help\n{top}"]
    commands = re.findall(r"^  ([a-z][a-z-]+)\s", top.split("Commands:")[1].split("Options:")[0], flags=re.M)
    for command in commands:
        if command in ("help", "devnet", "genesis"):
            continue
        parts.append(f"$ orien {command} --help\n{run(command, '--help')}")
    return "\n\n".join(parts)


def main() -> None:
    sections = []
    for heading, path, address in PAGES:
        text = page_text((ROOT / path).read_text())
        sections.append(f"# {heading}\nSource: {address}\n\n{text}")
    changelog = changelog_text((ROOT / "web/src/content/changelog.ts").read_text())
    sections.append(f"# Changelog, newest first\nSource: https://orien.network/changelog/\n\n{changelog}")
    # The running bot reads the website itself; from here on is what it cannot find there.
    sections.append("<!-- not on the website -->\n\n# Design notes: how coins are created (docs/epochs.md, written for developers)\n\n" + (ROOT / "docs/epochs.md").read_text().strip())
    cli = orien_help()
    if cli:
        sections.append("# The orien command-line program's own help\n\n" + cli)
    OUT.write_text("\n\n\n".join(sections) + "\n")
    print(f"wrote {OUT.relative_to(ROOT)}: {OUT.stat().st_size // 1024} KB")


if __name__ == "__main__":
    main()
