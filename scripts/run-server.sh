#!/usr/bin/env bash
# MCP server entry point for the Claude Code plugin. Uses an already installed
# ytdl-mcp binary from PATH, then hands stdin/stdout to it for JSON-RPC.
set -euo pipefail

binary="${YTDL_MCP_BIN:-ytdl-mcp}"

if ! command -v "${binary}" >/dev/null 2>&1; then
  printf 'ytdl-mcp plugin: ytdl-mcp is not installed or not on PATH.\n' >&2
  printf 'Install ytdl-mcp separately, then retry the plugin server.\n' >&2
  exit 127
fi

exec "${binary}" serve
