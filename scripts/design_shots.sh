#!/usr/bin/env bash
# Refresh design/ — one screenshot per reference screen, straight from the
# mock harness, so the folder always shows what the app currently looks like.
# (It used to hold frames captured from docs/design.html, which stopped
# matching the app.)
#
# Usage: scripts/design_shots.sh        — macOS only: `sips` converts BMP→PNG.
set -euo pipefail
cd "$(dirname "$0")/.."

OUT=design
WIN=${RDB_WIN:-1280x800}

# shot <screen> <name> [delay_ms] [theme]
shot() {
  local screen=$1 name=$2 delay=${3:-2800} theme=${4:-dark}
  RDB_MOCK=1 RDB_WIN="$WIN" RDB_THEME="$theme" RDB_SCREEN="$screen" \
    RDB_SHOT="$OUT/$name.bmp" RDB_SHOT_DELAY_MS="$delay" \
    cargo run -q -p rdb --features mock >/dev/null 2>&1
  sips -s format png "$OUT/$name.bmp" --out "$OUT/$name.png" >/dev/null
  rm -f "$OUT/$name.bmp"
  echo "$OUT/$name.png"
}

shot connections connections
shot workspace workspace
shot sql sql-editor 3200
shot sql-empty sql-editor-empty 3200
shot rail rail 4500
shot tab-menu tab-menu
shot modal-conn modal-open-connection
shot modal-db modal-open-database
shot palette command-palette
shot function function-view
shot settings-about settings-about
shot update-ready update-ready
shot whats-new whats-new

# A few in the light theme, since most of the palette work shows there.
shot connections connections-light 2800 light
shot workspace workspace-light 3200 light
shot sql sql-editor-light 3200 light
