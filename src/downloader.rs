//! yt-dlp invocation: build the argv, run the subprocess, collect produced
//! files + metadata. Ports `downloader.py`.
//!
//! yt-dlp does the heavy lifting; this module just constructs the right flags
//! and reads back what it produced via `--print after_move:`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Result};
use tokio::process::Command;

use crate::bootstrap::Tools;
use crate::model::{AudioFormat, DownloadMode, SearchResultItem, VideoContainer};
use crate::util::{command_error, is_http_url, json_str, run_capped, CommandOutput};
// Re-exported for `downloader_tests.rs`, which exercises the shared
// tail-truncation logic via this module's `super::*` glob.
#[cfg(test)]
pub(crate) use crate::util::stderr_tail_text;

mod probe;

pub use probe::{probe, ProbeResult};

/// Field separator embedded in the `--print` template (unit separator, unlikely
/// to appear in titles).
const SEP: char = '\u{1f}';
const STDERR_TAIL_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone)]
pub struct MediaFile {
    pub path: PathBuf,
    pub kind: &'static str, // "audio" | "video"
    pub size: u64,
    pub title: Option<String>,
    pub video_id: Option<String>,
    pub uploader: Option<String>,
    pub duration: Option<f64>,
}

#[derive(Debug, Clone, Default)]
pub struct ItemResult {
    pub url: String,
    pub title: Option<String>,
    pub video_id: Option<String>,
    pub uploader: Option<String>,
    pub duration: Option<f64>,
    pub is_playlist: bool,
    pub files: Vec<MediaFile>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct FetchOptions<'a> {
    pub mode: DownloadMode,
    pub staging: &'a Path,
    pub audio_format: AudioFormat,
    pub audio_quality: &'a str,
    pub container: VideoContainer,
    pub max_height: Option<u32>,
    pub archive_dir: Option<&'a Path>,
    pub timeout: Option<Duration>,
    pub clean_metadata: bool,
}

/// Non-greedy "Artist - Title" split applied to the title field so the artist
/// populates tags + the output folder. No-op when the title has no " - ".
const PARSE_ARTIST: &str = r"title:(?P<artist>.+?) - (?P<title>.+)";

/// Use the source playlist as the album tag when yt-dlp exposes one. This helps
/// full playlist downloads group cleanly in music libraries without guessing.
const PARSE_PLAYLIST_ALBUM: &str = r"playlist_title:%(album)s";
const TITLE_CLEANUPS: &[(&str, &str)] = &[
    (
        r"(?i)\s*[\[(](official\s+(music\s+)?video|official\s+audio|audio\s+only|lyric(s)?(\s+video)?|visuali[sz]er|music\s+video|hd|4k)[\])]\s*",
        "",
    ),
    (
        r"(?i)\s*[-–—]\s*(official\s+(music\s+)?video|official\s+audio|lyric(s)?(\s+video)?|visuali[sz]er|music\s+video)\s*$",
        "",
    ),
    (r"\s*[|｜]\s*@[\w.-]+\s*$", ""),
    (r"\s{2,}", " "),
    (r"^\s+|\s+$", ""),
];

/// Output template: per-kind subdir / Artist / Title [id].ext.
fn output_template(staging: &Path, kind: &str) -> String {
    format!(
        "{}/{kind}/%(artist,uploader,channel,creator|Unknown Artist)s/%(title)s [%(id)s].%(ext)s",
        staging.display()
    )
}

/// The `--print` template emitted once per produced file, after the final move.
fn print_template() -> String {
    format!("after_move:%(id)s{SEP}%(title)s{SEP}%(uploader)s{SEP}%(duration)s{SEP}%(filepath)s")
}

/// Append the end-of-options separator and the user-controlled positional.
///
/// end-of-options: a `--` before the positional prevents a '-'-prefixed value
/// (e.g. `--exec`, `--config-locations`, `-o`) from being parsed as a yt-dlp
/// flag → arbitrary command exec (F2). The positional is always the last argv
/// element and always preceded immediately by `--`.
fn positional_after_end_of_options(mut args: Vec<String>, positional: &str) -> Vec<String> {
    args.push("--".into());
    args.push(positional.to_string());
    args
}

/// Parse one `--print` line into `(id, title, uploader, duration, filepath)`.
/// Returns `None` for any line that doesn't have exactly the five expected
/// SEP-delimited fields (e.g. stray/malformed output), so callers can skip it.
fn parse_print_line(line: &str) -> Option<(&str, &str, &str, &str, &str)> {
    let parts: Vec<&str> = line.split(SEP).collect();
    if parts.len() != 5 {
        return None;
    }
    Some((parts[0], parts[1], parts[2], parts[3], parts[4]))
}

fn common_args(staging: &Path, kind: &str, tools: &Tools, archive: Option<&Path>) -> Vec<String> {
    let mut a = vec![
        "--quiet".into(),
        "--no-warnings".into(),
        "--no-progress".into(),
        "--windows-filenames".into(),
        "--parse-metadata".into(),
        PARSE_ARTIST.into(),
        "--parse-metadata".into(),
        PARSE_PLAYLIST_ALBUM.into(),
        "--write-info-json".into(),
        "--write-thumbnail".into(),
        "--write-description".into(),
        "--convert-thumbnails".into(),
        "jpg".into(),
        "-o".into(),
        output_template(staging, kind),
        "--print".into(),
        print_template(),
    ];
    if let Some(dir) = &tools.ffmpeg_dir {
        a.push("--ffmpeg-location".into());
        a.push(dir.display().to_string());
    }
    if let Some(extra) = &tools.extractor_args {
        a.push("--extractor-args".into());
        a.push(extra.clone());
    }
    if let Some(arch) = archive {
        a.push("--download-archive".into());
        a.push(arch.display().to_string());
    }
    a
}

fn add_metadata_cleanup_args(args: &mut Vec<String>) {
    for (pattern, replacement) in TITLE_CLEANUPS {
        args.extend([
            "--replace-in-metadata".into(),
            "title".into(),
            (*pattern).into(),
            (*replacement).into(),
        ]);
    }
}

fn audio_args(
    staging: &Path,
    fmt: AudioFormat,
    quality: &str,
    tools: &Tools,
    archive: Option<&Path>,
) -> Vec<String> {
    let mut a = common_args(staging, "audio", tools, archive);
    a.extend([
        "-f".into(),
        "bestaudio/best".into(),
        "--extract-audio".into(),
        "--audio-format".into(),
        fmt.as_str().into(),
    ]);
    if !fmt.is_lossless_or_passthrough() {
        a.push("--audio-quality".into());
        a.push(quality.to_string());
    }
    if fmt.is_taggable() {
        a.extend(["--embed-metadata".into(), "--embed-thumbnail".into()]);
    }
    a
}

fn video_args(
    staging: &Path,
    container: VideoContainer,
    max_height: Option<u32>,
    tools: &Tools,
    archive: Option<&Path>,
) -> Vec<String> {
    let mut a = common_args(staging, "video", tools, archive);
    let h = max_height
        .map(|h| format!("[height<=?{h}]"))
        .unwrap_or_default();
    a.extend([
        "-f".into(),
        format!("bv*{h}+ba/b{h}"),
        "--merge-output-format".into(),
        container.as_str().into(),
        "--embed-metadata".into(),
        "--embed-thumbnail".into(),
    ]);
    a
}

/// Run one yt-dlp pass, parsing the produced files + metadata from its
/// `--print` output straight into `result`. Fields are filled once (first
/// pass wins); `is_playlist` is set when a single pass yields >1 file.
async fn run_pass(
    ytdlp: &Path,
    url: &str,
    args: Vec<String>,
    kind: &'static str,
    timeout: Option<Duration>,
    result: &mut ItemResult,
) -> Result<()> {
    let argv = positional_after_end_of_options(args, url);
    let mut cmd = Command::new(ytdlp);
    cmd.args(&argv);
    let output = run_command(&mut cmd, timeout).await?;
    if !output.status.success() {
        bail!(
            "{}",
            command_error((output.stderr.as_str(), output.stdout.as_slice()))
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut count = 0;
    for line in stdout.lines() {
        let Some((id, title, uploader, duration, filepath)) = parse_print_line(line) else {
            continue;
        };
        result.video_id.get_or_insert_with(|| id.to_string());
        result.title.get_or_insert_with(|| title.to_string());
        if result.uploader.is_none() && uploader != "NA" {
            result.uploader = Some(uploader.to_string());
        }
        if result.duration.is_none() {
            result.duration = duration.parse().ok();
        }
        let path = PathBuf::from(filepath);
        if let Ok(md) = tokio::fs::metadata(&path).await {
            result.files.push(MediaFile {
                path,
                kind,
                size: md.len(),
                title: Some(title.to_string()),
                video_id: Some(id.to_string()),
                uploader: if uploader == "NA" {
                    None
                } else {
                    Some(uploader.to_string())
                },
                duration: duration.parse().ok(),
            });
            count += 1;
        }
    }
    if count > 1 {
        result.is_playlist = true;
    }
    Ok(())
}

/// Download one URL per `mode`, returning the per-URL outcome. `mode = both`
/// runs two passes (video then audio) into their own staging subdirs.
pub async fn fetch(tools: &Tools, url: &str, options: FetchOptions<'_>) -> ItemResult {
    let mut result = ItemResult {
        url: url.to_string(),
        ..Default::default()
    };

    if matches!(options.mode, DownloadMode::Video | DownloadMode::Both) {
        let archive = options.archive_dir.map(|d| d.join("archive-video.txt"));
        let mut args = video_args(
            options.staging,
            options.container,
            options.max_height,
            tools,
            archive.as_deref(),
        );
        if options.clean_metadata {
            add_metadata_cleanup_args(&mut args);
        }
        if let Err(e) = run_pass(
            &tools.ytdlp,
            url,
            args,
            "video",
            options.timeout,
            &mut result,
        )
        .await
        {
            result.error = Some(e.to_string());
            return result;
        }
    }
    if matches!(options.mode, DownloadMode::Audio | DownloadMode::Both) {
        let archive = options.archive_dir.map(|d| d.join("archive-audio.txt"));
        let mut args = audio_args(
            options.staging,
            options.audio_format,
            options.audio_quality,
            tools,
            archive.as_deref(),
        );
        if options.clean_metadata {
            add_metadata_cleanup_args(&mut args);
        }
        if let Err(e) = run_pass(
            &tools.ytdlp,
            url,
            args,
            "audio",
            options.timeout,
            &mut result,
        )
        .await
        {
            result.error = Some(e.to_string());
            return result;
        }
    }
    result
}

pub(crate) fn parse_search_json(bytes: &[u8]) -> Result<Vec<SearchResultItem>> {
    let info: serde_json::Value = serde_json::from_slice(bytes)?;
    let Some(entries) = info.get("entries").and_then(|entries| entries.as_array()) else {
        let keys = info
            .as_object()
            .map(|object| object.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        anyhow::bail!(
            "yt-dlp search JSON did not contain an entries array; top-level keys: {keys:?}"
        );
    };

    let results = entries
        .iter()
        .filter_map(search_result_item)
        .collect::<Vec<_>>();

    Ok(results)
}

fn search_result_item(entry: &serde_json::Value) -> Option<SearchResultItem> {
    if entry.is_null() {
        return None;
    }
    let title = json_str(entry, "title")?;
    let url = search_result_url(entry)?;
    Some(SearchResultItem {
        title,
        url,
        video_id: json_str(entry, "id"),
        uploader: json_str(entry, "uploader").or_else(|| json_str(entry, "channel")),
        duration: entry.get("duration").and_then(|d| d.as_f64()),
        thumbnail: json_str(entry, "thumbnail"),
        view_count: entry.get("view_count").and_then(|v| v.as_u64()),
    })
}

fn search_result_url(entry: &serde_json::Value) -> Option<String> {
    if let Some(url) = json_str(entry, "webpage_url") {
        return Some(url);
    }
    if let Some(url) = json_str(entry, "url").filter(|url| is_http_url(url)) {
        return Some(url);
    }
    json_str(entry, "id").map(|id| format!("https://www.youtube.com/watch?v={id}"))
}

pub(crate) fn search_spec(query: &str, limit: u32) -> String {
    format!(
        "ytsearch{}:{}",
        limit.clamp(1, crate::model::MAX_SEARCH_LIMIT),
        query.trim()
    )
}

pub async fn search_youtube(
    ytdlp: &Path,
    query: &str,
    limit: u32,
    extractor_args: Option<&str>,
    timeout: Option<Duration>,
) -> Result<Vec<SearchResultItem>> {
    let mut cmd = Command::new(ytdlp);
    cmd.args([
        "--dump-single-json",
        "--skip-download",
        "--no-warnings",
        "--quiet",
    ]);
    if let Some(extra) = extractor_args {
        cmd.arg("--extractor-args").arg(extra);
    }
    // end-of-options: the spec is prefixed with `ytsearch{N}:` so it can't lead
    // with '-', but keep the guard for defense-in-depth/consistency (F2).
    cmd.arg("--").arg(search_spec(query, limit));

    let output = run_command(&mut cmd, timeout).await?;
    if !output.status.success() {
        bail!(
            "{}",
            command_error((output.stderr.as_str(), output.stdout.as_slice()))
        );
    }

    parse_search_json(&output.stdout)
}

/// Run a yt-dlp subprocess via the shared runner, capping stderr to the last
/// 16 KiB. Thin wrapper kept for `downloader.rs`/`probe.rs` call sites.
async fn run_command(cmd: &mut Command, timeout: Option<Duration>) -> Result<CommandOutput> {
    run_capped(cmd, timeout, Some(STDERR_TAIL_BYTES)).await
}

#[cfg(test)]
#[path = "downloader_tests.rs"]
mod tests;
