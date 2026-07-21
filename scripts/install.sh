#!/usr/bin/env bash
set -euo pipefail

REPO="suiflex/rdb"
BINARY="rdb"
VERSION="${RDB_VERSION:-latest}"

log() {
  printf '==> %s\n' "$*"
}

fail() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || fail "missing required command: $1"
}

need_cmd curl
need_cmd tar
need_cmd unzip
need_cmd mktemp

OS_RAW="$(uname -s)"
ARCH_RAW="$(uname -m)"

case "$OS_RAW" in
  Darwin) OS="apple-darwin" ;;
  Linux) OS="unknown-linux-gnu" ;;
  *) fail "unsupported OS: $OS_RAW" ;;
esac

case "$ARCH_RAW" in
  x86_64|amd64) ARCH="x86_64" ;;
  arm64|aarch64) ARCH="aarch64" ;;
  *) fail "unsupported architecture: $ARCH_RAW" ;;
esac

TARGET_TRIPLE="${ARCH}-${OS}"
TARGET_HINT_1="$(printf '%s' "$OS_RAW" | tr '[:upper:]' '[:lower:]')"
TARGET_HINT_2="$ARCH_RAW"

if [ "$VERSION" = "latest" ]; then
  API_URL="https://api.github.com/repos/${REPO}/releases/latest"
else
  API_URL="https://api.github.com/repos/${REPO}/releases/tags/${VERSION}"
fi

log "Looking up ${REPO} release (${VERSION})"
RELEASE_JSON="$(curl -fsSL "$API_URL")" || fail "unable to fetch release metadata from ${API_URL}"

ASSET_URLS="$(printf '%s' "$RELEASE_JSON" | grep -o '"browser_download_url": *"[^"]*"' | cut -d '"' -f 4)"
[ -n "$ASSET_URLS" ] || fail "no downloadable assets found in release metadata"

pick_asset() {
  local url
  # On macOS prefer the .dmg: it carries the signed RDB.app that installs to
  # Applications (Launchpad). Other OSes take the tarball/zip (bare binary).
  if [ "$OS_RAW" = "Darwin" ]; then
    while IFS= read -r url; do
      case "$url" in
        *"$TARGET_TRIPLE"*.dmg|*apple-darwin*"$ARCH"*.dmg)
          printf '%s\n' "$url"
          return 0
          ;;
      esac
    done <<EOF
$ASSET_URLS
EOF
  fi
  while IFS= read -r url; do
    case "$url" in
      *"$TARGET_TRIPLE"*.tar.gz|*"$TARGET_TRIPLE"*.zip|*"$TARGET_HINT_1"*"$ARCH"*.tar.gz|*"$TARGET_HINT_1"*"$ARCH"*.zip|*"$TARGET_HINT_1"*"$TARGET_HINT_2"*.tar.gz|*"$TARGET_HINT_1"*"$TARGET_HINT_2"*.zip)
        printf '%s\n' "$url"
        return 0
        ;;
    esac
  done <<EOF
$ASSET_URLS
EOF
  return 1
}

ASSET_URL="$(pick_asset)" || fail "could not find a release asset matching ${TARGET_TRIPLE}"
ASSET_NAME="$(basename "$ASSET_URL")"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

ARCHIVE_PATH="$TMP_DIR/$ASSET_NAME"
EXTRACT_DIR="$TMP_DIR/extracted"
mkdir -p "$EXTRACT_DIR"

log "Downloading $ASSET_NAME"
curl -fsSL "$ASSET_URL" -o "$ARCHIVE_PATH"

case "$ASSET_NAME" in
  *.tar.gz) tar -xzf "$ARCHIVE_PATH" -C "$EXTRACT_DIR" ;;
  *.zip) unzip -q "$ARCHIVE_PATH" -d "$EXTRACT_DIR" ;;
  *.dmg)
    need_cmd hdiutil
    MNT="$TMP_DIR/mnt"
    mkdir -p "$MNT"
    hdiutil attach -nobrowse -readonly -mountpoint "$MNT" "$ARCHIVE_PATH" >/dev/null
    trap 'hdiutil detach "$MNT" >/dev/null 2>&1 || true; rm -rf "$TMP_DIR"' EXIT
    cp -R "$MNT"/*.app "$EXTRACT_DIR"/
    hdiutil detach "$MNT" >/dev/null
    trap 'rm -rf "$TMP_DIR"' EXIT
    ;;
  *) fail "unsupported asset format: $ASSET_NAME" ;;
esac

APP_PATH="$(find "$EXTRACT_DIR" -type d -name '*.app' | head -n 1)"
# A .app bundle wins on macOS (Launchpad); otherwise install the bare binary.
if [ -n "$APP_PATH" ]; then
  BIN_PATH=""
else
  BIN_PATH="$(find "$EXTRACT_DIR" -type f -name "$BINARY" | head -n 1)"
fi

if [ -n "$BIN_PATH" ]; then
  DEFAULT_INSTALL_DIR="$HOME/.local/bin"
  if [ -d "/usr/local/bin" ] && [ -w "/usr/local/bin" ]; then
    DEFAULT_INSTALL_DIR="/usr/local/bin"
  fi
  INSTALL_DIR="${INSTALL_DIR:-$DEFAULT_INSTALL_DIR}"
  mkdir -p "$INSTALL_DIR"

  DEST_PATH="$INSTALL_DIR/$BINARY"
  install -m 0755 "$BIN_PATH" "$DEST_PATH"

  log "Installed to $DEST_PATH"
  case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *)
      printf 'warning: %s is not on your PATH yet\n' "$INSTALL_DIR" >&2
      printf 'add this to your shell profile: export PATH="%s:$PATH"\n' "$INSTALL_DIR" >&2
      ;;
  esac
elif [ "$OS_RAW" = "Darwin" ] && [ -n "$APP_PATH" ]; then
  DEFAULT_APP_DIR="$HOME/Applications"
  if [ -d "/Applications" ] && [ -w "/Applications" ]; then
    DEFAULT_APP_DIR="/Applications"
  fi
  APP_INSTALL_DIR="${INSTALL_DIR:-$DEFAULT_APP_DIR}"
  mkdir -p "$APP_INSTALL_DIR"

  DEST_PATH="$APP_INSTALL_DIR/$(basename "$APP_PATH")"
  rm -rf "$DEST_PATH"
  cp -R "$APP_PATH" "$DEST_PATH"
  # Clear quarantine so the ad-hoc-signed app opens without a Gatekeeper prompt.
  xattr -dr com.apple.quarantine "$DEST_PATH" 2>/dev/null || true
  log "Installed app bundle to $DEST_PATH"

  # Keep the `rdb` terminal command working via a symlink into ~/.local/bin.
  CLI_DIR="$HOME/.local/bin"
  mkdir -p "$CLI_DIR"
  ln -sf "$DEST_PATH/Contents/MacOS/$BINARY" "$CLI_DIR/$BINARY"
  log "Linked CLI to $CLI_DIR/$BINARY"
  case ":$PATH:" in
    *":$CLI_DIR:"*) ;;
    *)
      printf 'warning: %s is not on your PATH yet\n' "$CLI_DIR" >&2
      printf 'add this to your shell profile: export PATH="%s:$PATH"\n' "$CLI_DIR" >&2
      ;;
  esac
else
  fail "downloaded archive does not contain $BINARY or a macOS .app bundle"
fi
