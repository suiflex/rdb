#!/usr/bin/env python3
"""Self-check for attribute_changelog.py. Run: python3 test_attribute_changelog.py"""

from attribute_changelog import attribute

# sha -> (login, name, email)
AUTHORS = {
    "aaa": ("ekacahya21", "Eka Cahya", "eka@example.com"),
    "bbb": ("wahyuakbarwibowo", "Wahyu Akbar", "wahyu@example.com"),
    "ccc": ("mulhamna", "Mulham", "mulham@example.com"),
    "ddd": (None, "Anon Person", "anon@example.com"),
    "eee": ("ekacahya21", "Eka Cahya", "eka@example.com"),
}

ROOT = """# Changelog

## RDB: [0.31.0](https://example.com/compare/v0.30.0...v0.31.0) (2026-08-03)


### App Features

* **app:** shiny new thing ([aaa](https://example.com/commit/aaaaaa))
* **app:** another new thing ([eee](https://example.com/commit/eeeeee))


### Bug Fixes

* **app:** fix a thing ([bbb](https://example.com/commit/bbbbbb))
* **ci:** a maintainer chore ([ccc](https://example.com/commit/cccccc))

## RDB: [0.30.0](https://example.com/compare/v0.29.0...v0.30.0) (2026-08-01)


### Bug Fixes

* **app:** something old ([999](https://example.com/commit/999999))
"""


def _resolve(sha):
    return AUTHORS[sha]


def test_appends_handle_before_the_commit_link():
    out = attribute(ROOT, resolve=_resolve, history=lambda t: {"eka@example.com", "wahyu@example.com"})
    assert "* **app:** shiny new thing (@ekacahya21) ([aaa]" in out, out
    assert "* **app:** fix a thing (@wahyuakbarwibowo) ([bbb]" in out, out


def test_maintainer_commits_are_left_untouched():
    out = attribute(ROOT, resolve=_resolve, history=lambda t: {"eka@example.com", "wahyu@example.com"})
    assert "* **ci:** a maintainer chore ([ccc](https://example.com/commit/cccccc))" in out
    assert "@mulhamna" not in out


def test_thanks_section_lists_non_maintainers_deduped():
    out = attribute(ROOT, resolve=_resolve, history=lambda t: {"eka@example.com", "wahyu@example.com"})
    top = out.split("## RDB: [0.30.0]")[0]
    assert "### Thanks" in top
    assert top.count("* @ekacahya21\n") == 1, top
    assert "* @wahyuakbarwibowo\n" in top


def test_new_contributors_are_those_absent_from_history():
    # only Eka is in prior history -> Wahyu and the login-less Anon are new
    root = ROOT.replace(
        "* **ci:** a maintainer chore ([ccc](https://example.com/commit/cccccc))",
        "* **app:** anon fix ([ddd](https://example.com/commit/dddddd))",
    )
    out = attribute(root, resolve=_resolve, history=lambda t: {"eka@example.com"})
    top = out.split("## RDB: [0.30.0]")[0]
    assert "### New Contributors" in top
    assert "* @wahyuakbarwibowo made their first contribution" in top
    assert "* Anon Person made their first contribution" in top
    assert "@ekacahya21 made their first contribution" not in top


def test_older_blocks_are_untouched():
    out = attribute(ROOT, resolve=_resolve, history=lambda t: set())
    tail = out.split("## RDB: [0.30.0]")[1]
    assert tail.strip() == (
        "(https://example.com/compare/v0.29.0...v0.30.0) (2026-08-01)\n\n\n"
        "### Bug Fixes\n\n"
        "* **app:** something old ([999](https://example.com/commit/999999))"
    )


def test_is_idempotent():
    once = attribute(ROOT, resolve=_resolve, history=lambda t: {"eka@example.com"})
    twice = attribute(once, resolve=_resolve, history=lambda t: {"eka@example.com"})
    assert once == twice


def test_maintainer_only_release_gets_no_sections():
    root = """# Changelog

## RDB: [0.31.0](https://example.com/compare/v0.30.0...v0.31.0) (2026-08-03)


### Bug Fixes

* **ci:** a maintainer chore ([ccc](https://example.com/commit/cccccc))
"""
    out = attribute(root, resolve=_resolve, history=lambda t: set())
    assert out == root
    assert "### Thanks" not in out


if __name__ == "__main__":
    for name, fn in sorted(globals().items()):
        if name.startswith("test_"):
            fn()
            print(f"ok {name}")
    print("all checks passed")
