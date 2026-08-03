#!/usr/bin/env python3
"""Self-check for fold-changelog.py. Run: python3 test_fold_changelog.py"""

from fold_changelog import fold

ROOT = """# Changelog

## RDB: [0.30.0](https://example.com/compare/v0.29.0...v0.30.0) (2026-08-01)


### App Features

* **app:** something old ([aaa](https://example.com/commit/aaa))

## RDB: [0.29.0](https://example.com/compare/v0.28.0...v0.29.0) (2026-07-01)


### Bug Fixes

* **app:** older still ([bbb](https://example.com/commit/bbb))
"""

GENERATED = """# Changelog

## [0.31.0](https://example.com/compare/v0.30.0...v0.31.0) (2026-08-03)


### Bug Fixes

* **ci:** the new thing ([ccc](https://example.com/commit/ccc))
"""


def test_prepends_under_header_and_prefixes_heading():
    out = fold(GENERATED, ROOT, "0.31.0", "RDB")
    assert out.startswith("# Changelog\n\n## RDB: [0.31.0]"), out[:80]
    assert "the new thing" in out
    # older releases survive, in order
    assert out.index("[0.31.0]") < out.index("[0.30.0]") < out.index("[0.29.0]")
    assert "something old" in out and "older still" in out


def test_replaces_stale_section_for_same_version():
    once = fold(GENERATED, ROOT, "0.31.0", "RDB")
    regenerated = GENERATED.replace("the new thing", "the regenerated thing")
    twice = fold(regenerated, once, "0.31.0", "RDB")
    assert twice.count("## RDB: [0.31.0]") == 1, twice
    assert "the regenerated thing" in twice
    assert "the new thing" not in twice
    assert "something old" in twice


def test_is_idempotent():
    once = fold(GENERATED, ROOT, "0.31.0", "RDB")
    twice = fold(GENERATED, once, "0.31.0", "RDB")
    assert once == twice


def test_rejects_a_section_for_the_wrong_version():
    try:
        fold(GENERATED, ROOT, "9.9.9", "RDB")
    except SystemExit:
        return
    raise AssertionError("expected a mismatched version to be rejected")


if __name__ == "__main__":
    for name, fn in sorted(globals().items()):
        if name.startswith("test_"):
            fn()
            print(f"ok {name}")
    print("all checks passed")
