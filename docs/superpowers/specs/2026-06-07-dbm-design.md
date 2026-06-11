# DBM — Lightweight Cross-Platform Database Manager

**Date:** 2026-06-07
**Status:** Design approved, pending spec review

## Goal

A native, lightweight database management GUI in the spirit of TablePlus —
fast, small binary, low memory — but built from a single Rust codebase that
targets macOS, Windows, and Linux (TablePlus uses 4 separate native codebases).

End-goal vision: a connection picker supporting ~20 engines (Postgres, MySQL,
Redis, Mongo, ClickHouse, BigQuery, SQLite, Oracle, Cassandra, etc.).

MVP ships **4 engines**: PostgreSQL, MySQL/MariaDB, Redis, MongoDB — chosen so
the driver abstraction is forced to handle both SQL (tabular) and NoSQL
(document, key-value) paradigms from day one.

## Why Rust

- C-level performance, no GC, no bundled runtime → light binary, low RAM
  (the actual source of TablePlus-style lightness is native rendering, not a
  specific language).
- Memory safety without a garbage collector.
- Mature async drivers exist for all 4 MVP engines.
- One codebase cross-compiles to all 3 desktop platforms.

## UI: Slint

Native, GPU-rendered UI via Slint. No webview, no Chromium → smaller and
lighter than Electron/Tauri, more native feel. Declarative `.slint` markup
keeps UI logic thin. Single codebase across platforms.

## Architecture

Cargo workspace (monorepo):

```
dbm/
├── Cargo.toml              # workspace root
├── crates/
│   ├── core/               # Driver trait + result model (paradigm-agnostic)
│   ├── driver-postgres/    # impl Driver via tokio-postgres
│   ├── driver-mysql/       # impl Driver via mysql_async
│   ├── driver-redis/       # impl Driver via redis
│   ├── driver-mongo/       # impl Driver via mongodb
│   └── connstore/          # saved connections + secrets (OS keychain)
└── app/                    # Slint UI binary
    └── ui/*.slint          # connection picker, query editor, result grid
```

**Core rule:** the UI depends only on `core::Driver`. It never imports a
concrete driver crate. Adding a new engine = a new `driver-*` crate that
implements the trait; the UI is untouched. This is what makes the 20-engine
vision cheap.

**Data flow:** UI → `Driver::query()` → `ResultSet` enum → UI renders by
variant (grid for Tabular, tree/JSON for Documents, key-list for KeyValue).

## Driver Trait + Result Model

```rust
#[async_trait]
pub trait Driver: Send + Sync {
    async fn connect(cfg: &ConnConfig) -> Result<Self> where Self: Sized;
    async fn ping(&self) -> Result<()>;
    async fn schema(&self) -> Result<Schema>;   // databases > containers > fields
    async fn query(&self, q: &Query) -> Result<ResultSet>;
    async fn close(self) -> Result<()>;
}

pub enum Query {
    Sql(String),              // PG, MySQL
    Command(Vec<String>),     // Redis: ["GET","key"]
    Mongo(MongoOp),           // find/insert/aggregate as a structured op
}

pub enum ResultSet {
    Tabular  { cols: Vec<Column>, rows: Vec<Row> },  // SQL results
    Documents(Vec<Json>),                            // Mongo
    KeyValue(Vec<(String, RedisValue)>),             // Redis
    Affected(u64),                                   // writes
}
```

- `Query` is an enum, not a string, so non-SQL engines (Redis, Mongo) are
  first-class and SQL assumptions never leak into the abstraction. Each driver
  handles the variant it understands and errors on the rest.
- `Schema` is unified: every engine maps to `databases → containers → fields`
  even when the native names differ (table / collection / keyspace), so the UI
  tree renders one way.

## Connection Security

- Connection metadata (host, port, user, db, sslmode) stored in plain config:
  `~/.config/dbm/connections.json` (platform config dir).
- **Passwords are never written to that file.** They go to the OS keychain via
  the `keyring` crate (macOS Keychain, Windows Credential Manager, Linux Secret
  Service). Config stores only a keychain reference id.
- Per-connection TLS / `sslmode` support.
- SSH tunnel support is explicitly **post-MVP**.

## Testing

- **Driver crates:** integration tests against real engines using the
  `testcontainers` crate (each driver spins its own PG/MySQL/Redis/Mongo Docker
  container, runs connect → query → assert). No mocks for drivers — real
  protocol behavior is the point.
- **core:** unit tests on `ResultSet` / `Schema` mapping logic; pure, no
  network.
- **app/UI:** manual testing for MVP. Slint logic is kept thin to minimize
  untested surface.

## Scope (MVP)

In:
- 4 engines (PG, MySQL, Redis, Mongo) behind one `Driver` trait.
- Connection picker, save/edit connections, keychain-backed passwords.
- Query editor + result rendering for all 3 result paradigms.
- Schema browser tree.
- Builds for macOS, Windows, Linux.

Out (post-MVP, noted not built):
- SSH tunnels.
- The remaining ~16 engines (added later, one driver crate each).
- Automated UI tests.
- Data export, query history, multi-tab beyond basics (revisit after MVP).

## UI Design Language

Aesthetic: **"precision instrument."** Refined-minimal, content-first — closer to
TablePlus / Linear / Things than a marketing page. This is a tool used for hours
daily, so predictability, density, and zero-friction beat visual drama. Design
rules below intentionally invert typical web-landing-page advice (no decorative
motion, no maximalist layout, system fonts preferred).

### Signature feature (steal from TablePlus)

Per-connection accent color. When saving a connection the user picks a color; it
tints the sidebar selection and a thin top window border. Instant "which database
am I in" — the main guard against running a destructive query on prod. This is the
one memorable, identity-giving element. Build it in the MVP.

### Design tokens (Slint global singleton)

All colors/spacing/typography live in one `global Theme` singleton so light/dark is
a single swap and nothing hardcodes values. Per-connection accent is a runtime
property fed in, not a static token.

| Token group | Values |
|---|---|
| Base (dark, default) | bg `#1b1d1f`, surface `#232629`, hairline `#ffffff14`, text `#e6e8ea`, text-dim `#8b9095` |
| Base (light) | bg `#fbfbfc`, surface `#ffffff`, hairline `#0000000d`, text `#1b1d1f`, text-dim `#6b7075` |
| Accent | per-connection, user-chosen (default `#3b82f6`); used for selection + top border only |
| Spacing | 4px grid: 4 / 8 / 12 / 16 / 24 |
| Row height | 30px (grid + tree rows) |
| Radius | 6px panels/modals, 4px controls |
| Elevation | one shadow only, on modals/popovers; panels use hairlines not shadows |
| Font (chrome) | system sans — SF Pro / Segoe UI / system Linux. **Embed nothing.** |
| Font (data cells, query editor) | system mono — SF Mono / Consolas / monospace |
| Font sizes | 13px body, 12px dim/labels, 13px mono data |

System fonts are a deliberate choice: native feel on each OS **and** zero added
binary weight (embedding a custom family adds ~200KB–1MB and looks non-native).

### Layout — fixed 3-pane

```
┌──────────────────────────────────────────────┐
│ ▔▔▔ accent top-border (per-connection color) ▔│
├───────────┬──────────────────────────────────┤
│ Sidebar   │ Tab bar (queries / open tables)   │
│           ├──────────────────────────────────┤
│ conns •   │                                   │
│ schema    │   Work area                       │
│  tree     │   (query editor → result grid /   │
│           │    tree / key-list by ResultSet)  │
│           │                                   │
├───────────┴──────────────────────────────────┤
│ status bar: conn name · latency · row count   │
└──────────────────────────────────────────────┘
```

Layout never rearranges — muscle memory is a feature. Sidebar collapsible; inspector
panel is post-MVP.

### Motion

Functional only. 80–120ms ease fades on hover, row select, tab switch. Nothing
decorative, no staggered reveals, no scroll effects. Heavy animation would hurt the
"light + fast" goal and drain battery in a daily-use tool.

### Keyboard-first (spec from day 1)

| Key | Action |
|---|---|
| Cmd/Ctrl-K | command palette (jump connection / table / action) |
| Cmd/Ctrl-Enter | run query |
| Cmd/Ctrl-1..9 | switch tab |
| Cmd/Ctrl-T | new query tab |
| j / k | move grid row selection |
| Cmd/Ctrl-W | close tab |

## Packaging & Binary Size

Target: **single binary < 15 MB, idle RAM < 60 MB.** Treat as a test, not a wish —
check the number per release.

### Size build flags (`Cargo.toml` release profile)

```toml
[profile.release]
opt-level = "z"      # optimize for size
lto = true           # link-time optimization
codegen-units = 1    # better optimization, smaller output
panic = "abort"      # drop unwind tables
strip = true         # strip symbols
```

Expect 30–50% smaller binary vs default release. Validate the < 15 MB target after
these are on; revisit if a driver pulls heavy deps.

### Slint renderer backend (RESOLVED)

Use **FemtoVG (OpenGL)** as the primary renderer. Skia renders prettiest but adds
significant binary weight; the software renderer is lightest but looks worst and is
CPU-bound. FemtoVG is the balance point for "light + good-looking" and is the direct
resolution of the size-vs-beauty tradeoff. Keep the software renderer available as a
fallback for machines without GL.

### Installers & signing (required, not optional)

Unsigned binaries are blocked by Gatekeeper (macOS) and SmartScreen (Windows), so
signing is part of MVP packaging, not a nicety.

| Platform | Format | Signing |
|---|---|---|
| macOS | `.dmg` | Developer ID sign **+ notarize** (else won't open) |
| Windows | `.msi` (or NSIS `.exe`) | Authenticode cert (else SmartScreen warning) |
| Linux | `.AppImage` (portable) + `.deb` | none required |

### Linux runtime dependency caveat

Keychain on Linux uses Secret Service, which needs a running provider
(gnome-keyring / KWallet via D-Bus). On a minimal install it may be absent — keeps
"light install" honest by handling it: detect at runtime, and if no Secret Service,
fall back to an encrypted local secrets file rather than crashing.

## Open Questions

- Connection-pool strategy per driver (single conn vs pool) — decide per driver
  during implementation.
- Command palette (Cmd-K) scope for MVP: connections + tables only, or also actions?
- Light vs dark as first-run default (leaning dark for the dev audience).
