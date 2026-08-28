# Vision

RDB is a **native, lightweight database editor for many engines** — one small
binary that speaks PostgreSQL, MySQL, MariaDB, Redis, Valkey, MongoDB, SQLite,
Cassandra, SQL Server, Oracle, and ClickHouse today, with room to grow.

The goal is simple: give people a fast, local-first tool that opens instantly,
stays out of the way, and handles a wide range of databases without shipping a
browser engine to do it.

## Why

Most cross-engine database GUIs are Electron apps: hundreds of megabytes, slow
to start, heavy on memory. The lightweight alternatives usually speak only one
engine. RDB aims to sit where those two miss each other — broad engine support
*and* a native footprint.

## Principles

- **Native & fast.** GPU-rendered UI via Slint, no webview. Aggressive release
  profile (`opt-level=z`, LTO, `panic=abort`, strip) keeps the binary small and
  startup instant.
- **Local-first & private.** Your data and queries stay on your machine.
  Connection secrets live in the OS keychain (AES-GCM encrypted-file fallback),
  never in plaintext config.
- **One tool, many engines.** A common `Driver` trait (`crates/core`) means the
  UI never hardcodes an engine beyond a single dispatch enum. Adding an engine
  is a new `driver-*` crate — the rest of the app doesn't change.
- **Boring where it counts.** Predictable behavior over clever features; a
  small, auditable codebase over a large one.

## Where it's headed

- More engines, added as independent `driver-*` crates behind the same trait.
- Deeper query ergonomics: filtering, export in common formats, charts.
- Quality-of-life for real day-to-day database work, kept lean.

Further out (aspirational, not committed yet):

- A community extension surface — let people add engines and panels the
  community builds and maintains, without forking the app. An agent panel is
  one of the things that could live here.

## Non-goals

- Not a hosted service — it's a desktop editor you run locally.
- No telemetry, no account, no cloud dependency to use the core app.

See [`CLAUDE.md`](./CLAUDE.md) for the current architecture and crate layout.
