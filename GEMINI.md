# ytdl-mcp

Download media from any [yt-dlp](https://github.com/yt-dlp/yt-dlp)-supported site
(YouTube, Vimeo, …) as audio, video, or both — with embedded metadata and cover
art — and rsync it to an SSH remote.

This extension provides two tools via the bundled `ytdl-mcp` MCP server:

- **`youtube_download`** — download one or more URLs and transfer them to the
  configured remote. Audio-first by default. Files are organized as
  `Artist/Title [id].ext` with title/artist/album/date tags and cover art
  embedded. Audio and video go to separate destinations.
- **`youtube_probe`** — read-only: resolve title/duration/uploader/format counts
  without downloading.

## Requirements

- The `ytdl-mcp` binary must be on your `PATH`. Install it by downloading the
  release for your platform from
  <https://github.com/jmagar/ytdl-mcp/releases>, or run `ytdl-mcp setup` once.
- `ssh` (and optionally `rsync`) for transfers. yt-dlp and ffmpeg are
  auto-downloaded by the binary on first use — you do not need to install them.

## Notes

- YouTube mix/radio URLs (`list=RD…`, `&start_radio=1`) are auto-cleaned to the
  seed video.
- Destinations and the SSH remote come from the extension settings
  (`YTDLP_REMOTE`, `YTDLP_REMOTE_PATH`, `YTDLP_VIDEO_REMOTE_PATH`).
