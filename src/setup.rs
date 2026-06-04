//! `ytdl-mcp setup` — interactive installer (Phase 3).
//!
//! Will: ensure yt-dlp + ffmpeg are installed, prompt for the remote/dest
//! config, detect which agent CLIs (claude/codex/gemini) are present, and
//! register this binary into the selected ones via their `mcp add` commands.

use anyhow::Result;

pub async fn run() -> Result<()> {
    eprintln!("ytdl-mcp setup is not implemented yet.");
    Ok(())
}
