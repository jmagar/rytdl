use std::path::Path;
use std::time::Duration;

use tokio::process::Command;

use super::run_command;
use crate::util::json_str;

#[derive(Debug, Clone, Default)]
pub struct ProbeResult {
    pub url: String,
    pub title: Option<String>,
    pub video_id: Option<String>,
    pub uploader: Option<String>,
    pub duration: Option<f64>,
    pub is_playlist: bool,
    pub entry_count: Option<usize>,
    pub format_count: Option<usize>,
    pub error: Option<String>,
}

/// Resolve metadata for a URL without downloading
/// (`yt-dlp -J --flat-playlist --skip-download`).
/// Takes just the yt-dlp path because probe never needs ffmpeg.
pub async fn probe(
    ytdlp: &Path,
    url: &str,
    extractor_args: Option<&str>,
    timeout: Option<Duration>,
) -> ProbeResult {
    let mut r = ProbeResult {
        url: url.to_string(),
        ..Default::default()
    };
    let mut cmd = Command::new(ytdlp);
    // `--flat-playlist` keeps the single-json dump LIGHTWEIGHT: when the URL is a
    // playlist, yt-dlp emits stub entries (id/title/url, no per-entry extraction)
    // instead of a full info dict per entry. The probe only needs the
    // playlist-vs-single-video shape, the entry count, and a few top-level fields,
    // so this avoids holding a multi-MB dump for large playlists (PerfL1) and is
    // faster. Flat mode is a no-op for a single video (no `entries` array).
    cmd.args([
        "-J",
        "--flat-playlist",
        "--skip-download",
        "--no-warnings",
        "--quiet",
    ]);
    if let Some(extra) = extractor_args {
        cmd.arg("--extractor-args").arg(extra);
    }
    // end-of-options: prevent a '-'-prefixed URL from being parsed as a yt-dlp
    // flag (e.g. --exec, --config-locations) → arbitrary command exec (F2).
    cmd.arg("--").arg(url);
    let output = match run_command(&mut cmd, timeout).await {
        Ok(o) => o,
        Err(e) => {
            r.error = Some(e.to_string());
            return r;
        }
    };
    if !output.status.success() {
        r.error = Some(output.stderr.trim().to_string());
        return r;
    }
    let info: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(v) => v,
        Err(e) => {
            r.error = Some(format!("could not parse yt-dlp JSON: {e}"));
            return r;
        }
    };

    let entries = info.get("entries").and_then(|e| e.as_array());
    if let Some(entries) = entries {
        r.is_playlist = true;
        // Prefer yt-dlp's top-level `playlist_count` (a single integer it provides
        // under flat mode) and fall back to counting non-null `entries` without
        // collecting a Vec of references. The first non-null entry is borrowed
        // lazily for the title/uploader fallbacks.
        let first_non_null = entries.iter().find(|e| !e.is_null());
        r.entry_count = info
            .get("playlist_count")
            .and_then(|c| c.as_u64())
            .map(|c| c as usize)
            .or_else(|| Some(entries.iter().filter(|e| !e.is_null()).count()));
        r.title = json_str(&info, "title")
            .or_else(|| first_non_null.and_then(|e| json_str(e, "playlist")));
        r.video_id = json_str(&info, "id");
        r.uploader = json_str(&info, "uploader")
            .or_else(|| first_non_null.and_then(|e| json_str(e, "uploader")));
    } else {
        r.title = json_str(&info, "title");
        r.video_id = json_str(&info, "id");
        r.uploader = json_str(&info, "uploader");
        r.duration = info.get("duration").and_then(|d| d.as_f64());
        r.format_count = info
            .get("formats")
            .and_then(|f| f.as_array())
            .map(|a| a.len())
            .filter(|n| *n > 0);
    }
    r
}
