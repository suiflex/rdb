#!/usr/bin/env bash
# Runs every RDB_SCREEN scenario and fails if the app dies on any of them.
#
# `cargo build` and `cargo test` both stayed green through a crash that aborted
# the app on a single click (a Slint layout re-entrancy panic under
# `panic = "abort"`), because neither of them ever runs the event loop. This
# does: each screen drives the real UI to a state, renders a frame, and quits.
# A screen that aborts, panics or never paints fails the run.
#
# Usage: scripts/danger_screens.sh          (needs `--features mock` built)
#   BIN=path/to/rdb                         override the binary
#   RDB_SHOT_DELAY_MS=6000                  give slow machines more room
#   DANGER_REQUIRE_FRAME=0                  skip the "did it paint" check
#
# On Linux run it under a display: SLINT_BACKEND=winit-software xvfb-run -a ...

# Same as design_shots.sh: the default BIN path is relative, so run from the
# repo root whatever directory the caller was in.
cd "$(dirname "$0")/.."

BIN=${BIN:-target/debug/rdb}
DELAY=${RDB_SHOT_DELAY_MS:-4000}
REQUIRE_FRAME=${DANGER_REQUIRE_FRAME:-1}
OUT=${DANGER_OUT:-$(mktemp -d)}

# Every screen `wire_screen_harness` knows about (app/src/wire/picker.rs).
# Adding an arm there means adding a line here — nothing enforces it, and a
# screen missing from this list is a scenario nobody runs.
SCREENS="
connections
workspace
workspace-users-bool
workspace-users-date
rail
sql
sql-empty
sql-select
sql-find
sql-multi
chart
function
palette
tab-menu
modal-conn
modal-db
modal-add-mongo
conn-add
export-menu
menu-hover
notch-light
sidebar-collapsed
shortcuts
tooltip
zoom
settings
settings-updates
settings-about
update-ready
update-install
update-installing
update-restarting
whats-new
workspace-active-commit
workspace-detail-commit
workspace-long-edit
workspace-pointer-edit
workspace-dirty
workspace-guard
workspace-commit
workspace-tabnav
workspace-filter
workspace-limit
workspace-insert
workspace-sql
workspace-tabflow
multi-connection
"

# Named screens override the list, for debugging one scenario:
#   scripts/danger_screens.sh update-install
if [ "$#" -gt 0 ]; then
  SCREENS="$*"
fi

if [ ! -x "$BIN" ]; then
  echo "no binary at $BIN — build it with: cargo build -p rdb --features mock" >&2
  exit 2
fi

mkdir -p "$OUT"
echo "logs and frames in $OUT"

failed=""
for screen in $SCREENS; do
  log="$OUT/$screen.log"
  rc=0
  # multi-connection drives two connects, a table open, a tab switch and a new
  # tab before it asserts; the default delay shoots the frame mid-scenario.
  delay=$DELAY
  case "$screen" in multi-connection) [ "$DELAY" -lt 12000 ] && delay=12000;; esac
  (
    export RDB_MOCK=1
    export RDB_SCREEN="$screen"
    export RDB_WIN=1280x800
    export RDB_SHOT="$OUT/$screen.bmp"
    export RDB_SHOT_DELAY_MS="$delay"
    # RDB_STORE_DIR is not optional: without it the run reads and overwrites
    # the developer's real connection store *and* their open query tabs.
    export RDB_STORE_DIR="$OUT/store-$screen"
    exec "$BIN"
  ) >"$log" 2>&1 &
  app=$!
  # Hand-rolled watchdog rather than `timeout`, which macOS does not ship. The
  # app quits itself once it has its frame; this only catches a hang.
  ( sleep 120; kill -9 "$app" 2>/dev/null ) &
  watchdog=$!
  wait "$app" || rc=$?
  kill "$watchdog" 2>/dev/null
  wait "$watchdog" 2>/dev/null

  if [ "$rc" -ne 0 ]; then
    # 137 is the watchdog's: the app hung instead of reaching its own quit.
    echo "FAIL $screen (exit $rc)"
    failed="$failed $screen"
    continue
  fi
  # A panic on a worker thread can leave the exit code at 0.
  if grep -qE "panicked|Recursion detected" "$log"; then
    echo "FAIL $screen (panic in log)"
    failed="$failed $screen"
    continue
  fi
  # Exiting cleanly without painting means the screen proved nothing.
  if [ "$REQUIRE_FRAME" = "1" ] && ! grep -q "RDB_SHOT saved" "$log"; then
    echo "FAIL $screen (no frame rendered)"
    failed="$failed $screen"
    continue
  fi
  echo "ok   $screen"
done

if [ -n "$failed" ]; then
  # Every failure, not just the first: one red run should show everything that
  # broke.
  for screen in $failed; do
    echo
    echo "----- $screen -----"
    tail -20 "$OUT/$screen.log"
  done
  echo
  echo "failed:$failed"
  exit 1
fi

echo "all screens ok"
