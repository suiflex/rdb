# Contributing to RDBS

Thanks for your interest in RDBS — a native, cross-platform database manager
(PostgreSQL, MySQL, Redis, MongoDB, and more) built with Rust and Slint.

This guide covers how to build, test, and submit changes. By participating you
agree to abide by our [Code of Conduct](CODE_OF_CONDUCT.md).

## Getting started

RDBS is a Cargo workspace:

- `app/` — the Slint UI binary (`rdbs`), the only crate that names a concrete
  driver (in `app/src/dispatch.rs`).
- `crates/core/` — the `Driver` trait, `Query`, `ResultSet`, `Schema`, errors.
- `crates/connstore/` — saved connections + OS keychain / AES-GCM.
- `crates/driver-*/` — one crate per engine, each implementing `Driver`.

You need a stable Rust toolchain with `rustfmt` and `clippy`:

```bash
rustup component add rustfmt clippy
```

UI builds need the Slint system libraries for your platform — see the
[Slint prerequisites](https://releases.slint.dev/latest/docs/rust/slint/#prerequisites).

## Build, lint, test

A root `Makefile` wraps the common Cargo invocations and splits the frontend
(the `rdbs` UI binary) from the backend (the `crates/*` libraries). Run
`make help` for the full list. The most common targets:

```bash
make fe-build     # build the rdbs UI
make fe-run       # run the UI
make be-build     # build backend crates only
make be-test      # test backend crates only
make fmt-check    # format check (make fmt to apply)
make lint         # clippy, warnings treated as errors
make test         # test the whole workspace
make all          # fmt-check + lint + test + build (the CI gate)
```

Run `make all` before opening a pull request. Integration tests
(`tests/integration.rs`) need Docker and run locally via `make test-it`; they
are intentionally kept out of CI.

## Adding a database engine

1. Create a new `crates/driver-<engine>/` crate that implements the `Driver`
   trait from `rdbs-core`.
2. Add a variant to the `AnyDriver` enum in `app/src/dispatch.rs` and forward
   the trait methods.

Keep engine-specific logic (SQL strings, protocol quirks) inside the driver
crate. The app layer must stay engine-agnostic — the only place it names a
concrete driver is `AnyDriver`.

## Commit conventions

We follow [Conventional Commits](https://www.conventionalcommits.org/); the
`release-please` workflow parses them to drive the changelog and version bumps.

- Types: `feat`, `fix`, `chore`, `refactor`, `docs`, `test`, `build`, `ci`.
- Subject line ≤ 72 chars, imperative mood, no trailing period.
- Wrap the body at 72 chars and explain **why** the change exists — the diff
  already shows the what.
- One logical change per commit. Each commit should leave the tree in a
  buildable, testable state so `git revert` stays safe.
- Do **not** hand-edit the `release-please`-managed sections of any changelog.

## Branching & pull requests

- Branch off `develop` using a name that matches the leading commit type:
  `feat/…`, `fix/…`, `refactor/…`, `chore/…`, `docs/…`.
- Fill out the pull request template (Summary / Changes / Test plan).
- Keep PRs focused on one logical change — smaller PRs are easier to review.
- CI runs a scoped workflow per component (a `core` change also retests its
  dependents). Make sure `make all` passes locally first.

## Reporting bugs & requesting features

Use the issue forms under **Issues → New issue**. Security vulnerabilities must
**not** be filed as public issues — see [SECURITY.md](SECURITY.md).

## License

By contributing, you agree that your contributions are licensed under the
[MIT License](LICENSE) that covers this project.
