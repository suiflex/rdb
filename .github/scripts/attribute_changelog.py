#!/usr/bin/env python3
"""Add contributor attribution to the newest release block of CHANGELOG.md.

Runs right after fold_changelog.py in the release-please workflow, on the
pending release branch. It rewrites only the topmost
`## <product>: [x.y.z]` block:

  * each bullet gains ` (@handle)` after the subject (before the commit
    link), resolved from the commit's GitHub author. Commits authored by a
    maintainer are left untouched so that work reads as plain maintainer
    work.
  * a `### Thanks` section lists every non-maintainer contributor in the
    block (returning contributors included).
  * a `### New Contributors` section welcomes anyone whose commit email is
    not present in git history before the previous release tag.

Re-runnable: a block that already carries a `### Thanks` section is returned
unchanged. fold_changelog.py rebuilds the block from scratch on every run, so
in the workflow this always re-derives the attribution from a clean block.
"""

import os
import re
import subprocess
import sys

MAINTAINERS = {"mulhamna", "badrus123"}

HEADING = "# Changelog"
_HEAD_RE = re.compile(r"^## (?:.+?: )?\[([^\]]+)\]\(([^)]*)\)")
_PREVTAG_RE = re.compile(r"/compare/(.+?)\.\.\.")
_BULLET_RE = re.compile(
    r"^(\* (?:\*\*[^:*]+:\*\* )?)(.*?)( \(\[[0-9a-f]+\]\([^)]*\)\))\s*$"
)
_SHA_RE = re.compile(r"\[([0-9a-f]+)\]")


def _split_top_block(text):
    """Return (before, top_block, after) splitting on `## ` release headings.

    `before` keeps the `# Changelog` title and any preamble; `top_block` is
    the newest release section; `after` is every older section.
    """
    parts = re.split(r"^(?=## )", text, flags=re.MULTILINE)
    head = parts[0] if parts and not parts[0].startswith("## ") else ""
    sections = parts[1:] if head else parts
    if not sections:
        return text, "", ""
    return head, sections[0], "".join(sections[1:])


def gh_author(sha):
    """(login, name, email) for a commit sha, via `gh api`. All may be None.

    ponytail: one gh api call per bullet, cached by sha. A release is a
    handful of commits; batch via the GraphQL API if that ever stops being
    true.
    """
    repo = os.environ.get("GITHUB_REPOSITORY", "suiflex/rdb")
    proc = subprocess.run(
        [
            "gh", "api", f"repos/{repo}/commits/{sha}",
            "--jq", "[.author.login, .commit.author.name, .commit.author.email]"
            " | @tsv",
        ],
        capture_output=True,
        text=True,
    )
    if proc.returncode != 0:
        return (None, None, None)
    cols = (proc.stdout.rstrip("\n").split("\t") + ["", "", ""])[:3]
    return tuple(c or None for c in cols)


def prev_emails(prevtag):
    """Author emails in git history up to and including `prevtag`."""
    if not prevtag:
        return set()
    proc = subprocess.run(
        ["git", "log", prevtag, "--format=%ae"],
        capture_output=True,
        text=True,
    )
    if proc.returncode != 0:
        return set()
    return {line.strip() for line in proc.stdout.splitlines() if line.strip()}


def _token(login, name):
    """Display token for a contributor: `@login` when known, else the name."""
    if login:
        return f"@{login}"
    return name or "an unknown contributor"


def attribute(text, resolve=gh_author, history=prev_emails, product="RDB"):
    before, block, after = _split_top_block(text)
    if not block:
        return text
    if "### Thanks" in block or "### New Contributors" in block:
        return text  # already attributed this run

    lines = block.splitlines()
    head_match = _HEAD_RE.match(lines[0])
    if not head_match:
        return text
    prevtag_match = _PREVTAG_RE.search(head_match.group(2))
    prevtag = prevtag_match.group(1) if prevtag_match else None

    cache = {}
    contributors = []  # ordered, de-duped display tokens (non-maintainers)
    seen_tokens = set()
    release_emails = {}  # email -> display token, non-maintainers only

    out_lines = []
    for line in lines:
        m = _BULLET_RE.match(line)
        if not m:
            out_lines.append(line)
            continue
        prefix, subject, link = m.group(1), m.group(2), m.group(3)
        sha = _SHA_RE.search(link).group(1)
        if sha not in cache:
            cache[sha] = resolve(sha)
        login, name, email = cache[sha]

        is_maintainer = bool(login) and login.lower() in MAINTAINERS
        if is_maintainer:
            out_lines.append(line)
            continue

        token = _token(login, name)
        out_lines.append(f"{prefix}{subject} ({token}){link}")
        if token not in seen_tokens:
            seen_tokens.add(token)
            contributors.append(token)
        if email:
            release_emails.setdefault(email, token)

    if not contributors:
        return text  # nothing to credit (maintainer-only release)

    known = history(prevtag)
    newcomers = [
        token
        for email, token in release_emails.items()
        if email not in known
    ]

    block = "\n".join(out_lines).rstrip("\n")
    block += "\n\n\n### Thanks\n\n"
    block += "Thanks to everyone who contributed to this release:\n\n"
    block += "\n".join(f"* {t}" for t in contributors) + "\n"
    if prevtag and newcomers:
        block += "\n\n### New Contributors\n\n"
        block += "\n".join(
            f"* {t} made their first contribution" for t in newcomers
        ) + "\n"

    rebuilt = before + block.rstrip("\n")
    if after:
        rebuilt += "\n\n" + after
    return rebuilt.rstrip("\n") + "\n"


def main():
    dst = sys.argv[1] if len(sys.argv) > 1 else "CHANGELOG.md"
    product = os.environ.get("PRODUCT_NAME", "RDB")
    if not os.path.exists(dst):
        print(f"{dst} not present; nothing to attribute")
        return
    with open(dst, encoding="utf-8") as fh:
        text = fh.read()
    out = attribute(text, product=product)
    if out == text:
        print("no attribution changes")
        return
    with open(dst, "w", encoding="utf-8") as fh:
        fh.write(out)
    print(f"attributed contributors in {dst}")


if __name__ == "__main__":
    main()
