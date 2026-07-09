# ytdl-mcp

Node launcher for the `ytdl-mcp` Rust MCP server and CLI binary.

```bash
npx -y ytdl-mcp
```

Run the guided setup:

```bash
npx -y ytdl-mcp setup
```

Install globally when you want the command on `PATH`:

```bash
npm i -g ytdl-mcp
ytdl-mcp --version
ytdl-mcp setup
```

The package downloads the matching GitHub Release binary during `postinstall`.
The npm package version and the `ytdl-mcp` release tag are expected to match.
Release automation publishes this package from the repository `v*` tag workflow;
the GitHub repository must have an `NPM_TOKEN` secret with publish access.

## MCP stdio

Run without subcommands, `ytdl-mcp` serves MCP over stdio. MCP clients can launch
it with:

```json
{
  "command": "npx",
  "args": ["-y", "ytdl-mcp"],
  "env": {
    "YTDLP_REMOTE": "tootie",
    "YTDLP_REMOTE_PATH": "/media/music"
  }
}
```

## Overrides

```bash
YTDL_MCP_BINARY_VERSION=v0.7.0 npm i -g ytdl-mcp
YTDL_MCP_RELEASE_BASE_URL=https://github.com/jmagar/ytdl-mcp/releases/download npm i -g ytdl-mcp
YTDL_MCP_SKIP_DOWNLOAD=1 npm i -g ytdl-mcp
```

Supported binary targets are Linux x64 and Windows x64, matching the current
GitHub Release assets.
