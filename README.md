# ytdl-mcp

A cross-platform, single-binary **MCP server** that downloads media from any
[yt-dlp](https://github.com/yt-dlp/yt-dlp)-supported site (YouTube, Vimeo, …) as
**audio, video, or both** — defaulting to audio — embeds metadata + cover art,
organizes files as `Artist/Title [id].ext`, and `rsync`s (or `scp`s) the result
to a directory on an SSH remote you have passwordless key-based auth for.

Written in Rust (built on the [`rmcp`](https://crates.io/crates/rmcp) crate).
**yt-dlp and ffmpeg are auto-downloaded** into a per-user cache on first run, so
the host needs neither pre-installed.

> The original Python implementation lives under [`python/`](./python) for
> reference; the Rust binary supersedes it.

## Tools

| Tool | Purpose |
| --- | --- |
| `youtube_download` | Download one or more URLs (audio/video/both) and rsync/scp them to a remote dir. |
| `youtube_probe` | Read-only: resolve title/duration/uploader/format counts without downloading. |

`youtube_download` cleans YouTube mix/radio URLs to the seed video, tracks a
per-mode download archive (`use_archive`), sends audio and video to **separate**
destinations, and keeps the local staging copy on transfer failure for retry.

## Install

Download the binary for your platform from
[Releases](https://github.com/jmagar/ytdl-mcp/releases), or build it:

```bash
cargo build --release      # target/release/ytdl-mcp
```

Then run the guided installer, which fetches yt-dlp + ffmpeg, prompts for your
remote/destinations, and registers the server into whichever agent CLIs you have:

```bash
ytdl-mcp setup
```

It shells out to each CLI's own `mcp add` (Claude Code, Codex, Gemini CLI) — no
JSON/TOML hand-editing.

### Manual registration

Run bare, the binary serves MCP over stdio. Register it yourself with:

```bash
claude mcp add -s user ytdl-mcp -e YTDLP_REMOTE=tootie -e YTDLP_REMOTE_PATH=/media/music -- /path/to/ytdl-mcp
codex  mcp add --env YTDLP_REMOTE=tootie --env YTDLP_REMOTE_PATH=/media/music ytdl-mcp -- /path/to/ytdl-mcp
gemini mcp add -s user ytdl-mcp /path/to/ytdl-mcp -e YTDLP_REMOTE=tootie -e YTDLP_REMOTE_PATH=/media/music
```

It is also distributed as a **Claude Code plugin** (`.claude-plugin/`, which
downloads the release binary) and a **Gemini CLI extension**
(`gemini-extension.json`).

## Configuration (environment variables)

| Var | Default | Meaning |
| --- | --- | --- |
| `YTDLP_REMOTE` | — | SSH remote (alias or user@host) for transfers. |
| `YTDLP_REMOTE_PATH` | — | Absolute remote dir for **audio**. |
| `YTDLP_VIDEO_REMOTE_PATH` | falls back to audio | Absolute remote dir for **video**. |
| `YTDLP_AUDIO_FORMAT` | `mp3` | Default audio codec (`mp3`/`m4a`/`opus`/`flac`/`wav`/`best`). |
| `YTDLP_STAGING_DIR` | system temp | Local staging base dir. |
| `YTDLP_SSH_OPTS` | — | Extra ssh options (appended after `-o BatchMode=yes -o StrictHostKeyChecking=accept-new`). |
| `YTDLP_ARCHIVE_DIR` | per-user state dir | Where `use_archive` history lives. |
| `YTDLP_AUTO_UPDATE` | `1` | Re-download yt-dlp when stale. |
| `YTDLP_MAX_AGE_DAYS` | `14` | Staleness threshold (days). |
| `YTDLP_UPDATE_PRE` | `0` | Track yt-dlp's nightly channel. |
| `YTDLP_PATH` / `FFMPEG_PATH` | — | Use a specific yt-dlp / ffmpeg instead of auto-download. |

## Requirements

- **ssh** (and optionally **rsync** — falls back to **scp**, e.g. on Windows).
- Passwordless key-based SSH auth to the remote.
- yt-dlp and ffmpeg are fetched automatically (override with `YTDLP_PATH` /
  `FFMPEG_PATH`, or just have them on `PATH`).

## Build from source / cross-compile

```bash
cargo build --release                                   # Linux
cargo xwin build --release --target x86_64-pc-windows-msvc   # Windows (needs nasm + llvm)
cargo test && cargo clippy --all-targets -- -D warnings
```
