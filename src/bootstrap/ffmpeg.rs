//! Resolve or install ffmpeg.
//!
//! Uses ffmpeg-sidecar's low-level download/unpack pointed at our own cache dir
//! (not its `auto_download`, which targets the executable's own directory).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use ffmpeg_sidecar::download::{download_ffmpeg_package, ffmpeg_download_url, unpack_ffmpeg};

use super::{exe_name, resolve_override_or_path};
use crate::config::Config;

pub fn ensure(bin_dir: &Path, cfg: &Config) -> Result<PathBuf> {
    let pin = cfg.ffmpeg_sha256.as_deref();
    // 1. override / 2. PATH
    if let Some(p) = resolve_override_or_path(cfg.ffmpeg_path.as_deref(), "FFMPEG_PATH", "ffmpeg")?
    {
        super::verify_pin(&p, pin, "ffmpeg")?;
        return Ok(p);
    }
    // 3. cache
    let cached = bin_dir.join(exe_name("ffmpeg"));
    if cached.is_file() {
        super::verify_pin_cached(&cached, pin, "ffmpeg")?;
        return Ok(cached);
    }
    // 4. download + unpack into the cache dir
    let url = ffmpeg_download_url().context("no ffmpeg build for this platform")?;
    tracing::info!(%url, "downloading ffmpeg");
    let archive = download_ffmpeg_package(url, bin_dir).context("download ffmpeg package")?;
    unpack_ffmpeg(&archive, bin_dir).context("unpack ffmpeg")?;
    if cached.is_file() {
        // verify_pin_cached removes the unpacked binary on mismatch so a
        // tampered build isn't cached and trusted next run. (ffmpeg-sidecar's
        // unpack already sets the executable bit; we can't reorder before it,
        // but the file is removed on failure.)
        super::verify_pin_cached(&cached, pin, "ffmpeg")?;
        Ok(cached)
    } else {
        anyhow::bail!("ffmpeg not found at {} after unpack", cached.display())
    }
}
