use std::path::Path;
use std::time::Duration;

use tokio::process::Command;

use super::{run_command, str_field};

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

/// Resolve metadata for a URL without downloading (`yt-dlp -J --skip-download`).
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
    cmd.args(["-J", "--skip-download", "--no-warnings", "--quiet"]);
    if let Some(extra) = extractor_args {
        cmd.arg("--extractor-args").arg(extra);
    }
    cmd.arg(url);
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
        let non_null: Vec<&serde_json::Value> = entries.iter().filter(|e| !e.is_null()).collect();
        r.is_playlist = true;
        r.entry_count = Some(non_null.len());
        r.title = str_field(&info, "title")
            .or_else(|| non_null.first().and_then(|e| str_field(e, "playlist")));
        r.video_id = str_field(&info, "id");
        r.uploader = str_field(&info, "uploader")
            .or_else(|| non_null.first().and_then(|e| str_field(e, "uploader")));
    } else {
        r.title = str_field(&info, "title");
        r.video_id = str_field(&info, "id");
        r.uploader = str_field(&info, "uploader");
        r.duration = info.get("duration").and_then(|d| d.as_f64());
        r.format_count = info
            .get("formats")
            .and_then(|f| f.as_array())
            .map(|a| a.len())
            .filter(|n| *n > 0);
    }
    r
}
