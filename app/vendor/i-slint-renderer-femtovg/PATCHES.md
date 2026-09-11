# Local patches to i-slint-renderer-femtovg

Vendored from crates.io `i-slint-renderer-femtovg` **1.17.1** (the version
`slint 1.17.1` pins exactly) and wired in through `[patch.crates-io]` in the
root `Cargo.toml`. Every change is marked with an `rdb patch` comment.

## Layers drawn on whole pixels (`itemrenderer.rs`)

A rounded `clip` (and `opacity`, `cache-rendering-hint`) renders its children
into an offscreen texture that is then blitted back. Upstream renders and
blits it at `logical origin × scale factor` without rounding. At a fractional
scale factor — which the app's ⌘+/⌘− zoom produces — that origin lands between
pixels, so every blit resamples the texture and everything inside a rounded
card, field, chip or menu turns blurry while unclipped text stays sharp.

- `render_into_layer`: translate by the **floored** physical origin.
- `visit_clip`, `render_and_blend_layer`: blit at the same floored origin.
- `create_layer_target`: allocate one spare pixel per axis for the fraction
  dropped by the floor (zero-sized layers still return `None`).

## Upgrading Slint

1. Replace this directory with the new release's crate source
   (`~/.cargo/registry/src/*/i-slint-renderer-femtovg-<version>/`), dropping
   `.cargo-ok`, `.cargo_vcs_info.json` and `Cargo.lock`.
2. Re-apply the `rdb patch` hunks above (or drop them if upstream now snaps
   layer origins itself).
3. Check with the zoom harness: `RDB_MOCK=1 RDB_SCREEN=zoom RDB_SHOT=… cargo
   run -p rdb --features mock` — text inside the sidebar card must be as sharp
   as text outside it.
