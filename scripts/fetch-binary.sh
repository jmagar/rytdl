#!/usr/bin/env bash
# Idempotently download the ytdl-mcp release binary into the plugin's persistent
# data dir. Safe to run repeatedly: only downloads when the binary is missing.
# All output goes to stderr so it never pollutes the MCP stdio channel.
set -euo pipefail

DATA="${CLAUDE_PLUGIN_DATA:?CLAUDE_PLUGIN_DATA not set}"
BIN_DIR="$DATA/bin"
BIN="$BIN_DIR/ytdl-mcp"
REPO="jmagar/ytdl-mcp"

log() { echo "[ytdl-mcp] $*" >&2; }

# Resolve the plugin's own version from .claude-plugin/plugin.json (sibling of
# this script's scripts/ dir) so we fetch the binary that matches this plugin
# revision rather than whatever "latest" happens to be. Falls back to "latest"
# (a versioned tag URL of `latest` is not valid, so we signal it specially) when
# the manifest can't be parsed, so installs never hard-fail on a parse error.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MANIFEST="$SCRIPT_DIR/../.claude-plugin/plugin.json"

plugin_version() {
  [ -f "$MANIFEST" ] || return 1
  if command -v jq >/dev/null 2>&1; then
    jq -r '.version // empty' "$MANIFEST" 2>/dev/null
  else
    # Grab the first "version": "X.Y.Z" value without jq.
    sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$MANIFEST" \
      | head -n1
  fi
}

VERSION="$(plugin_version || true)"
if [ -n "$VERSION" ]; then
  release_path="download/v$VERSION"
  log "resolving binary for plugin version v$VERSION"
else
  release_path="latest/download"
  log "could not read version from $MANIFEST; falling back to latest release"
fi

if [ -x "$BIN" ]; then
  exit 0
fi

mkdir -p "$BIN_DIR"

os="$(uname -s)"
arch="$(uname -m)"
case "$os/$arch" in
  Linux/x86_64) asset="ytdl-mcp-linux-x86_64" ;;
  *)
    log "no prebuilt binary for $os/$arch. Download or build from https://github.com/$REPO/releases and place it at $BIN"
    exit 1
    ;;
esac

url="https://github.com/$REPO/releases/$release_path/$asset"

fetch() { # fetch <url> <out>; prints to stdout via -O- when out is "-"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$1" -o "$2"
  else
    wget -qO "$2" "$1"
  fi
}

# download to a temp file, verify, then atomically move into place
tmp="$BIN.part"
log "downloading $asset"
if ! fetch "$url" "$tmp"; then
  if [ "$release_path" != "latest/download" ]; then
    rm -f "$tmp"
    release_path="latest/download"
    url="https://github.com/$REPO/releases/$release_path/$asset"
    log "no exact v$VERSION release asset found; falling back to latest release"
    fetch "$url" "$tmp"
  else
    rm -f "$tmp"
    exit 1
  fi
fi

# Verify against the release's published checksum. Current releases built by
# .github/workflows/release.yml publish <asset>.sha256, so a missing checksum is
# treated as an install failure. Set YTDL_MCP_ALLOW_MISSING_CHECKSUM=1 only for
# compatibility testing with older/manual releases that predate checksum files.
expected=$(fetch "$url.sha256" - 2>/dev/null | awk '{print $1}') || true
if [ -n "$expected" ]; then
  actual=$(sha256sum "$tmp" | awk '{print $1}')
  if [ "$expected" != "$actual" ]; then
    rm -f "$tmp"
    log "checksum mismatch for $asset (expected $expected, got $actual) — refusing to install"
    exit 1
  fi
  log "checksum verified"
else
  if [ "${YTDL_MCP_ALLOW_MISSING_CHECKSUM:-}" = "1" ]; then
    log "no published checksum for $asset; continuing because YTDL_MCP_ALLOW_MISSING_CHECKSUM=1"
  else
    rm -f "$tmp"
    log "no published checksum for $asset; refusing to install"
    log "set YTDL_MCP_ALLOW_MISSING_CHECKSUM=1 only for compatibility testing with older/manual releases"
    exit 1
  fi
fi

chmod 0755 "$tmp"
mv -f "$tmp" "$BIN"
log "ready: $BIN"
