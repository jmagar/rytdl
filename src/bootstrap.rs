//! Locate (and, in a later phase, auto-install) the external binaries the
//! server shells out to: yt-dlp and ffmpeg.
//!
//! Phase 1: resolve from an explicit env override or the PATH. Phase 2 will add
//! auto-download into a per-user cache dir so the host needs neither
//! pre-installed.

use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::config::Config;

/// Resolved paths to the external tools used by a download.
#[derive(Debug, Clone)]
pub struct Tools {
    pub ytdlp: PathBuf,
    /// Directory containing the ffmpeg binary (passed to yt-dlp via
    /// `--ffmpeg-location`), or None to let yt-dlp find ffmpeg itself.
    pub ffmpeg_dir: Option<PathBuf>,
}

/// Resolve yt-dlp + ffmpeg. Errors with an actionable message if yt-dlp is
/// missing (ffmpeg is optional here — yt-dlp will error later if a download
/// actually needs it and it's absent).
pub fn resolve(cfg: &Config) -> Result<Tools> {
    let ytdlp = resolve_one("yt-dlp", cfg.ytdlp_path.as_deref()).context(
        "yt-dlp not found. Set YTDLP_PATH, put yt-dlp on PATH, or run `ytdl-mcp setup`.",
    )?;
    let ffmpeg = resolve_one("ffmpeg", cfg.ffmpeg_path.as_deref()).ok();
    let ffmpeg_dir = ffmpeg.and_then(|p| p.parent().map(PathBuf::from));
    Ok(Tools { ytdlp, ffmpeg_dir })
}

/// Resolve a single binary: explicit override first, else PATH lookup.
fn resolve_one(name: &str, override_path: Option<&str>) -> Result<PathBuf> {
    if let Some(p) = override_path {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return Ok(pb);
        }
        anyhow::bail!("{name} override path does not exist: {p}");
    }
    which::which(name).with_context(|| format!("`{name}` not found on PATH"))
}
