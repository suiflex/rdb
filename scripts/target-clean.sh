#!/usr/bin/env bash
# Safe, opt-in Cargo target/ storage audit and stale-artifact cleanup for the RDB workspace.
# Preserves all active third-party dependency caches (.rlib/.rmeta/.dylib) within the retention
# window (--days N) so switching between `cargo check`, `cargo build`, `cargo test`, `clippy`,
# and `--features mock` never recompiles third-party crates, while immediately reclaiming:
#   1. Unpacked .rcgu.o object files in target/*/deps/
#   2. Superseded workspace crate binaries/rlibs/rmetas (rdb, rdb-*)
#   3. Superseded/duplicate incremental compilation directories (target/*/incremental/<crate>-*)
#   4. Third-party dependency artifacts and incremental sessions older than --days N
#
# Usage:
#   ./scripts/target-clean.sh [--dry-run] [--days N] [--yes]

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT_DIR/target}"

DRY_RUN=0
DAYS=7
ASSUME_YES=0

while [[ $# -gt 0 ]]; do
    case "$1" in
        --dry-run|-n)
            DRY_RUN=1
            shift
            ;;
        --days)
            DAYS="${2:?--days requires a number}"
            shift 2
            ;;
        --yes|-y)
            ASSUME_YES=1
            shift
            ;;
        --help|-h)
            sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            echo "Unknown option: $1" >&2
            exit 2
            ;;
    esac
done

if [[ ! -d "$TARGET_DIR" ]]; then
    echo "No target directory at $TARGET_DIR"
    exit 0
fi

# Guard against active cargo processes holding locks in target/
if pgrep -f "cargo .*${ROOT_DIR}" >/dev/null 2>&1; then
    echo "Active cargo process detected in $ROOT_DIR; refusing to modify $TARGET_DIR." >&2
    exit 1
fi

echo "=== Target Storage Report ($TARGET_DIR) ==="
du -sh "$TARGET_DIR" "$TARGET_DIR"/debug "$TARGET_DIR"/release \
    "$TARGET_DIR"/debug/deps "$TARGET_DIR"/debug/incremental "$TARGET_DIR"/debug/build \
    2>/dev/null || true

CANDIDATES_FILE="$(mktemp)"
RLIBS_FILE="$(mktemp)"
EXISTING_FILE="$(mktemp)"
trap 'rm -f "$CANDIDATES_FILE" "$RLIBS_FILE" "$EXISTING_FILE"' EXIT

# 1. Unpacked .rcgu.o object files left in target/*/deps/ (superseded by split-debuginfo = "off")
find "$TARGET_DIR" -path "*/deps/*.rcgu.o" -type f 2>/dev/null >> "$CANDIDATES_FILE" || true

# 2. Fingerprint-driven superseded hash detection for workspace crates (`rdb*`, `i-slint-renderer-femtovg`):
#    Keeps the newest .rlib and .rmeta hash per (crate, kind, target, path, compile_kind, profile)
#    while leaving third-party dependency caches untouched unless older than --days N.
find "$TARGET_DIR" -path "*/deps/*.rlib" -type f 2>/dev/null > "$RLIBS_FILE" || true

find "$TARGET_DIR" \( -path "*/.fingerprint/rdb-*/*.json" -o -path "*/.fingerprint/i-slint-renderer-femtovg-*/*.json" \) \
    -type f -print0 2>/dev/null \
    | xargs -0 stat -f "%m|%N" 2>/dev/null \
    | sort -t'|' -k1,1nr \
    | awk -F'|' -v rlibs_file="$RLIBS_FILE" '
        BEGIN {
            while ((getline rpath < rlibs_file) > 0) {
                rlibs[rpath] = 1
            }
            close(rlibs_file)
        }
        {
            file = $2
            line = ""
            if ((getline line < file) <= 0) {
                close(file)
                next
            }
            close(file)
            n = split(file, p, "/")
            if (n < 4) next
            prof_dir = ""
            for (i = 1; i <= n - 3; i++) prof_dir = (i == 1 ? p[i] : prof_dir "/" p[i])
            dir_name = p[n - 1]
            kind = p[n]
            base = dir_name
            sub(/-[0-9a-f]{16}$/, "", base)
            hash = substr(dir_name, length(base) + 2)
            u_base = base
            gsub(/-/, "_", u_base)
            target = ""; path = ""; ckind = ""; prof = ""
            if (match(line, /"target":[0-9]+/)) target = substr(line, RSTART+9, RLENGTH-9)
            if (match(line, /"path":[0-9]+/)) path = substr(line, RSTART+7, RLENGTH-7)
            if (match(line, /"compile_kind":[0-9]+/)) ckind = substr(line, RSTART+15, RLENGTH-15)
            if (match(line, /"profile":[0-9]+/)) prof = substr(line, RSTART+10, RLENGTH-10)
            has_rlib = ((prof_dir "/deps/lib" u_base "-" hash ".rlib") in rlibs) ? "rlib" : "rmeta"
            key = prof_dir "|" base "|" kind "|" target "|" path "|" ckind "|" prof "|" has_rlib
            if (seen[key]++) {
                print prof_dir "/.fingerprint/" dir_name
                print prof_dir "/build/" dir_name
                print prof_dir "/deps/" u_base "-" hash
                print prof_dir "/deps/" u_base "-" hash ".d"
                print prof_dir "/deps/lib" u_base "-" hash ".rlib"
                print prof_dir "/deps/lib" u_base "-" hash ".rmeta"
                print prof_dir "/deps/lib" u_base "-" hash ".dylib"
                print prof_dir "/deps/lib" u_base "-" hash ".d"
            }
        }
    ' 2>/dev/null >> "$CANDIDATES_FILE" || true

# 3. Incremental cache deduplication:
#    - Keep only the newest <crate>-<hash> directory per crate under target/*/incremental/
#    - Drop incremental sessions and stale third-party deps older than --days N
find "$TARGET_DIR" -path "*/incremental/*" -mindepth 3 -maxdepth 3 -type d -print0 2>/dev/null \
    | xargs -0 stat -f "%m|%N" 2>/dev/null \
    | sort -t'|' -k1,1nr \
    | awk -F'|' '
        {
            dir = $2
            n = split(dir, p, "/")
            base = p[n]
            sub(/-[0-9a-z]{13}$/, "", base)
            prof_dir = ""
            for (i = 1; i <= n - 2; i++) {
                prof_dir = (i == 1 ? p[i] : prof_dir "/" p[i])
            }
            key = prof_dir "|" base
            if (seen[key]++) {
                print dir
            }
        }
    ' >> "$CANDIDATES_FILE" || true

find "$TARGET_DIR" -path "*/incremental/*/*" -maxdepth 4 -mtime +"$DAYS" 2>/dev/null >> "$CANDIDATES_FILE" || true
find "$TARGET_DIR" -path "*/deps/*" -maxdepth 3 -type f -mtime +"$DAYS" 2>/dev/null >> "$CANDIDATES_FILE" || true

# Filter to existing paths and deduplicate
sort -u "$CANDIDATES_FILE" | awk '{ if (system("test -e \"" $0 "\"") == 0) print $0 }' > "$EXISTING_FILE"

COUNT="$(grep -c . "$EXISTING_FILE" || true)"
if [[ "$COUNT" -eq 0 ]]; then
    echo "No stale artifacts found."
    exit 0
fi

TOTAL_KB="$(tr '\n' '\0' < "$EXISTING_FILE" | xargs -0 du -sk 2>/dev/null | awk '{sum += $1} END {print sum+0}')"
TOTAL_MB="$(awk -v kb="$TOTAL_KB" 'BEGIN {printf "%.1f", kb / 1024.0}')"

echo ""
echo "Found $COUNT reclaimable stale artifact path(s) (~${TOTAL_MB} MB)."

if [[ "$DRY_RUN" -eq 1 ]]; then
    echo "Dry-run mode (--dry-run): no files removed."
    exit 0
fi

if [[ "$ASSUME_YES" -ne 1 ]]; then
    printf "Remove these %d stale artifact path(s) (~%s MB)? [y/N] " "$COUNT" "$TOTAL_MB"
    read -r ans
    case "$ans" in
        [yY]|[yY][eE][sS]) ;;
        *)
            echo "Aborted."
            exit 0
            ;;
    esac
fi

tr '\n' '\0' < "$EXISTING_FILE" | xargs -0 rm -rf

echo ""
echo "=== Post-Cleanup Target Storage ($TARGET_DIR) ==="
du -sh "$TARGET_DIR" "$TARGET_DIR"/debug "$TARGET_DIR"/release \
    "$TARGET_DIR"/debug/deps "$TARGET_DIR"/debug/incremental "$TARGET_DIR"/debug/build \
    2>/dev/null || true
