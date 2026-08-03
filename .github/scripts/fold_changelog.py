#!/usr/bin/env python3
"""Fold release-please's package changelog into the repo-root CHANGELOG.md.

release-please resolves `changelog-path` relative to its package directory
and rejects `..`, so it can't be pointed at the repo root directly. Instead
it writes its default `app/CHANGELOG.md` and this script splices that
section into the root file, prefixes the heading with the product name, and
removes the package copy so the changelog only ever lives at the root.

Re-runnable: release-please recreates its release branch as new commits land
on develop, so a section for the same version is replaced rather than
duplicated or left stale.
"""

import os
import re
import sys

HEADING = "# Changelog"


def split_sections(body):
    """Split markdown into (preamble, [section, ...]) on `## ` headings."""
    parts = re.split(r"^(?=## )", body, flags=re.MULTILINE)
    if not parts:
        return "", []
    if parts[0].startswith("## "):
        return "", parts
    return parts[0], parts[1:]


def strip_top_heading(text):
    """Drop a leading `# ...` title and the blank lines under it."""
    lines = text.splitlines(keepends=True)
    if lines and lines[0].startswith("# "):
        lines = lines[1:]
        while lines and not lines[0].strip():
            lines = lines[1:]
    return "".join(lines)


def fold(src_text, dst_text, version, product):
    section = strip_top_heading(src_text).strip("\n")
    if not section:
        raise SystemExit(f"no changelog section found for {version}")

    # Target the exact released version, not "whichever heading is first" —
    # a rebased release branch can otherwise carry an unrelated block on top.
    marker = f"## [{version}]"
    prefixed = f"## {product}: [{version}]"
    if section.startswith(marker):
        section = prefixed + section[len(marker):]
    elif not section.startswith(prefixed):
        raise SystemExit(
            f"generated section does not start with a {version} heading"
        )

    preamble, sections = split_sections(strip_top_heading(dst_text))
    # Drop any existing block for this version so a regenerated release
    # branch replaces it instead of stacking a second copy.
    sections = [
        s
        for s in sections
        if not (s.startswith(marker) or s.startswith(prefixed))
    ]

    body = "".join(sections).lstrip("\n")
    out = f"{HEADING}\n\n{section}\n\n"
    if preamble.strip():
        out = f"{HEADING}\n\n{preamble.strip()}\n\n{section}\n\n"
    if body:
        out += body
    return out.rstrip("\n") + "\n"


def main():
    version = os.environ["RELEASED_VERSION"]
    product = os.environ.get("PRODUCT_NAME", "RDB")
    src = sys.argv[1] if len(sys.argv) > 1 else "app/CHANGELOG.md"
    dst = sys.argv[2] if len(sys.argv) > 2 else "CHANGELOG.md"

    if not os.path.exists(src):
        print(f"{src} not present; nothing to fold")
        return

    with open(src, encoding="utf-8") as fh:
        src_text = fh.read()
    with open(dst, encoding="utf-8") as fh:
        dst_text = fh.read()

    with open(dst, "w", encoding="utf-8") as fh:
        fh.write(fold(src_text, dst_text, version, product))
    print(f"folded {version} into {dst}")


if __name__ == "__main__":
    main()
