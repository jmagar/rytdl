use std::{ffi::OsString, path::PathBuf, sync::OnceLock};

use super::*;
use crate::identify::{IdentifyPayload, IdentifyResult, RetagPreview, TagWriteResult};
use crate::model::{AudioFormat, DownloadMode, ResponseFormat, SearchInput, Urls, VideoContainer};

fn test_config() -> Config {
    Config {
        remote: None,
        dest_path: None,
        video_dest_path: None,
        staging_dir: None,
        audio_format: "mp3".into(),
        ssh_opts: vec![],
        archive_dir: None,
        history_path: None,
        plex_url: None,
        plex_token: None,
        plex_playlist: None,
        clean_metadata: true,
        acoustid_client_key: None,
        fpcalc_path: None,
        musicbrainz_contact: None,
        auto_update: false,
        max_age_days: 14,
        update_pre: false,
        ytdlp_path: None,
        ffmpeg_path: None,
        extractor_args: None,
        ytdlp_sha256: None,
        ffmpeg_sha256: None,
        ytdlp_timeout_secs: 5,
        transfer_timeout_secs: 5,
    }
}

fn download_input(urls: Urls) -> DownloadInput {
    DownloadInput {
        urls,
        mode: DownloadMode::Audio,
        audio_format: None,
        audio_quality: "0".into(),
        max_height: None,
        container: VideoContainer::Mp4,
        remote: None,
        dest_path: None,
        video_dest_path: None,
        keep_local: false,
        use_archive: false,
        plex_playlist: None,
        response_format: ResponseFormat::Markdown,
    }
}

static PATH_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

async fn path_lock() -> tokio::sync::MutexGuard<'static, ()> {
    PATH_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await
}

#[tokio::test]
async fn run_download_json_appends_history_entry_with_destination_and_files() {
    let _guard = path_lock().await;

    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    let staging = dir.path().join("staging");
    let history = dir.path().join("downloads.jsonl");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&staging).unwrap();
    let fake = write_fake_runtime(&bin);

    let _path = PathOverride::prepend(bin.clone());

    let mut cfg = test_config();
    cfg.ytdlp_path = Some(fake.ytdlp.display().to_string());
    cfg.ffmpeg_path = Some(fake.ffmpeg.display().to_string());
    cfg.staging_dir = Some(staging.display().to_string());
    cfg.history_path = Some(history.display().to_string());

    let input = DownloadInput {
        mode: DownloadMode::Video,
        remote: Some("media".into()),
        dest_path: Some("/audio".into()),
        video_dest_path: Some("/video".into()),
        response_format: ResponseFormat::Json,
        ..download_input(Urls::One(
            "https://www.youtube.com/watch?v=abc123&list=RDfake".into(),
        ))
    };

    let output = run_download(&cfg, input).await.unwrap();
    let payload: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(payload["transferred"], true);

    let lines = std::fs::read_to_string(&history).unwrap();
    let entries = lines.lines().collect::<Vec<_>>();
    assert_eq!(entries.len(), 1);
    let entry: serde_json::Value = serde_json::from_str(entries[0]).unwrap();

    assert!(entry["timestamp"].as_str().unwrap().contains('T'));
    assert_eq!(entry["mode"], "video");
    assert_eq!(entry["remote"], "media");
    assert_eq!(entry["transferred"], true);
    assert_eq!(entry["total_files"], 1);
    assert_eq!(entry["destinations"][0]["kind"], "video");
    assert_eq!(entry["items"][0]["status"], "ok");
    assert_eq!(
        entry["items"][0]["url"],
        "https://www.youtube.com/watch?v=abc123"
    );
    assert_eq!(entry["items"][0]["files"][0]["kind"], "video");
}

#[tokio::test]
async fn run_download_json_reports_history_error_without_failing_download() {
    let _guard = path_lock().await;

    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    let staging = dir.path().join("staging");
    let bad_parent = dir.path().join("not-a-dir");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::write(&bad_parent, "file blocks history parent").unwrap();
    let fake = write_fake_runtime(&bin);

    let _path = PathOverride::prepend(bin.clone());

    let mut cfg = test_config();
    cfg.ytdlp_path = Some(fake.ytdlp.display().to_string());
    cfg.ffmpeg_path = Some(fake.ffmpeg.display().to_string());
    cfg.staging_dir = Some(staging.display().to_string());
    cfg.history_path = Some(bad_parent.join("downloads.jsonl").display().to_string());

    let input = DownloadInput {
        mode: DownloadMode::Video,
        remote: Some("media".into()),
        dest_path: Some("/audio".into()),
        video_dest_path: Some("/video".into()),
        response_format: ResponseFormat::Json,
        ..download_input(Urls::One("https://www.youtube.com/watch?v=abc123".into()))
    };

    let output = run_download(&cfg, input).await.unwrap();
    let payload: serde_json::Value = serde_json::from_str(&output).unwrap();

    assert_eq!(payload["transferred"], true);
    assert_eq!(payload["total_files"], 1);
    assert!(payload["history_error"]
        .as_str()
        .unwrap()
        .contains("create history directory"));
}

#[tokio::test]
async fn run_search_json_uses_fake_ytdlp_and_records_effective_args() {
    let dir = tempfile::tempdir().unwrap();
    let ytdlp = write_fake_search_ytdlp(dir.path(), "args.txt");

    let mut cfg = test_config();
    cfg.ytdlp_path = Some(ytdlp.display().to_string());
    cfg.extractor_args = Some("youtube:player_client=android".into());

    let output = run_search(
        &cfg,
        SearchInput {
            query: "  slow pulp live  ".into(),
            limit: 100,
            response_format: ResponseFormat::Json,
        },
    )
    .await
    .unwrap();

    let value: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(value["query"], "slow pulp live");
    assert_eq!(value["limit"], 25);
    assert_eq!(
        value["results"][0]["url"],
        "https://www.youtube.com/watch?v=fake123"
    );

    let args = std::fs::read_to_string(dir.path().join("args.txt")).unwrap();
    assert!(args.contains("--extractor-args"));
    assert!(args.contains("youtube:player_client=android"));
    assert!(args.contains("ytsearch25:slow pulp live"));
}

#[tokio::test]
async fn run_search_rejects_empty_query_before_tool_resolution() {
    let mut cfg = test_config();
    cfg.ytdlp_path = Some("/definitely/not/a/yt-dlp".into());

    let err = run_search(
        &cfg,
        SearchInput {
            query: "   ".into(),
            limit: 10,
            response_format: ResponseFormat::Json,
        },
    )
    .await
    .unwrap_err()
    .to_string();

    assert_eq!(err, "Search query cannot be empty.");
}

#[tokio::test]
async fn invalid_transfer_target_is_rejected_before_tool_resolution() {
    let mut cfg = test_config();
    cfg.ytdlp_path = Some("/definitely/not/a/yt-dlp".into());
    cfg.ffmpeg_path = Some("/definitely/not/a/ffmpeg".into());

    let input = DownloadInput {
        remote: Some("-bad".into()),
        dest_path: Some("/music".into()),
        response_format: ResponseFormat::Json,
        ..download_input(Urls::One("https://example.test/watch".into()))
    };

    let err = run_download(&cfg, input).await.unwrap_err().to_string();

    assert!(err.contains("SSH remote must not start with '-'"));
    assert!(!err.contains("YTDLP_PATH"));
    assert!(!err.contains("FFMPEG_PATH"));
}

#[tokio::test]
async fn run_download_json_reports_partial_status_with_fake_runtime() {
    let _guard = path_lock().await;

    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    let staging = dir.path().join("staging");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&staging).unwrap();
    let fake = write_fake_runtime(&bin);

    let _path = PathOverride::prepend(bin.clone());

    let mut cfg = test_config();
    cfg.ytdlp_path = Some(fake.ytdlp.display().to_string());
    cfg.ffmpeg_path = Some(fake.ffmpeg.display().to_string());
    cfg.staging_dir = Some(staging.display().to_string());

    let input = DownloadInput {
        mode: DownloadMode::Both,
        audio_format: Some(AudioFormat::Mp3),
        remote: Some("media".into()),
        dest_path: Some("/audio".into()),
        video_dest_path: Some("/video".into()),
        response_format: ResponseFormat::Json,
        ..download_input(Urls::One("https://example.test/watch".into()))
    };

    let output = run_download(&cfg, input).await;

    let value: serde_json::Value = serde_json::from_str(&output.unwrap()).unwrap();
    assert_eq!(value["transferred"], true);
    assert_eq!(value["partial_items"], 1);
    assert_eq!(value["failed_items"], 0);
    assert_eq!(value["total_files"], 1);
    assert_eq!(value["destinations"][0]["kind"], "video");
    assert_eq!(value["items"][0]["status"], "partial");
    assert_eq!(value["items"][0]["error"], "audio pass failed");
    assert_eq!(value["items"][0]["files"][0]["kind"], "video");
}

#[tokio::test]
async fn auto_retag_audio_paths_writes_when_acoustid_is_configured() {
    let mut cfg = test_config();
    cfg.acoustid_client_key = Some("test-key".into());
    let paths = vec!["/tmp/song.mp3".to_string()];
    let expected_paths = paths.clone();

    let summary = auto_retag_audio_paths_for_test(&cfg, paths, move |_cfg, paths| {
        let expected_paths = expected_paths.clone();
        Box::pin(async move {
            assert_eq!(paths, expected_paths);
            Ok(IdentifyPayload {
                results: vec![IdentifyResult {
                    path: "/tmp/song.mp3".into(),
                    duration: Some(185),
                    candidates: Vec::new(),
                    retag_preview: Some(RetagPreview {
                        confidence: 0.98,
                        recording_id: "recording-1".into(),
                        recording_title: "Song".into(),
                        artist: "Artist".into(),
                        artists: vec!["Artist".into()],
                        release_id: None,
                        release_title: Some("Album".into()),
                        release_group_id: None,
                        release_group_title: None,
                        release_type: None,
                        release_date: Some("2026".into()),
                        track_number: Some("1".into()),
                        musicbrainz_url: "https://musicbrainz.org/recording/recording-1".into(),
                    }),
                    retag_preview_error: None,
                    tag_write: Some(TagWriteResult {
                        written: true,
                        fields: vec!["artist".into(), "title".into()],
                    }),
                    tag_write_error: None,
                    error: None,
                }],
            })
        })
    })
    .await
    .expect("auto retag summary");

    assert_eq!(summary["attempted"], 1);
    assert_eq!(summary["matched"], 1);
    assert_eq!(summary["written"], 1);
    assert_eq!(summary["errors"], 0);
}

#[cfg(unix)]
fn write_fake_search_ytdlp(dir: &std::path::Path, args_file: &str) -> PathBuf {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    let ytdlp = dir.join("yt-dlp");
    let args_path = dir.join(args_file);
    let mut file = std::fs::File::create(&ytdlp).unwrap();
    write!(
        file,
        r#"#!/bin/sh
set -eu
printf '%s\n' "$*" > '{}'
cat <<'JSON'
{{"entries":[{{"id":"fake123","title":"Fake Search Result","url":"fake123","uploader":"Fake Channel","duration":187}}]}}
JSON
"#,
        args_path.display()
    )
    .unwrap();
    file.sync_all().unwrap();
    drop(file);
    let mut perms = std::fs::metadata(&ytdlp).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&ytdlp, perms).unwrap();
    ytdlp
}

#[cfg(windows)]
fn write_fake_search_ytdlp(dir: &std::path::Path, args_file: &str) -> PathBuf {
    let ytdlp = dir.join("yt-dlp.cmd");
    let args_path = dir.join(args_file);
    std::fs::write(
        &ytdlp,
        format!(
            "@echo %* > \"{}\"\r\n@echo {{\"entries\":[{{\"id\":\"fake123\",\"title\":\"Fake Search Result\",\"url\":\"fake123\",\"uploader\":\"Fake Channel\",\"duration\":187}}]}}\r\n",
            args_path.display()
        ),
    )
    .unwrap();
    ytdlp
}

struct PathOverride {
    old_path: Option<OsString>,
}

impl PathOverride {
    fn prepend(path: PathBuf) -> Self {
        let old_path = std::env::var_os("PATH");
        let mut path_entries = vec![path];
        if let Some(old_path) = &old_path {
            path_entries.extend(std::env::split_paths(old_path));
        }

        std::env::set_var("PATH", std::env::join_paths(path_entries).unwrap());

        Self { old_path }
    }
}

impl Drop for PathOverride {
    fn drop(&mut self) {
        if let Some(old_path) = self.old_path.take() {
            std::env::set_var("PATH", old_path);
        } else {
            std::env::remove_var("PATH");
        }
    }
}

struct FakeRuntime {
    ytdlp: PathBuf,
    ffmpeg: PathBuf,
}

#[cfg(unix)]
fn write_fake_runtime(bin: &std::path::Path) -> FakeRuntime {
    use std::os::unix::fs::PermissionsExt;

    let ytdlp = bin.join("yt-dlp");
    std::fs::write(
        &ytdlp,
        r#"#!/bin/sh
set -eu
out=""
while [ "$#" -gt 0 ]; do
  if [ "$1" = "-o" ]; then
    shift
    out="$1"
  fi
  shift || true
done
case "$out" in
  */video/*)
    staging="${out%%/video/*}"
    file="$staging/video/Fake Artist/Fake Title [vid123].mp4"
    mkdir -p "$(dirname "$file")"
    printf "video bytes" > "$file"
    printf 'vid123\037Fake Title\037Fake Artist\03712.5\037%s\n' "$file"
    ;;
  */audio/*)
    printf 'audio pass failed\n' >&2
    exit 33
    ;;
  *)
    printf 'unexpected output template: %s\n' "$out" >&2
    exit 34
    ;;
esac
"#,
    )
    .unwrap();
    let ffmpeg = bin.join("ffmpeg");
    std::fs::write(&ffmpeg, b"#!/bin/sh\nexit 0\n").unwrap();
    let ssh = bin.join("ssh");
    std::fs::write(&ssh, b"#!/bin/sh\nexit 0\n").unwrap();
    let rsync = bin.join("rsync");
    std::fs::write(&rsync, b"#!/bin/sh\nexit 0\n").unwrap();
    for path in [&ytdlp, &ffmpeg, &ssh, &rsync] {
        let mut perms = std::fs::metadata(path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms).unwrap();
    }
    FakeRuntime { ytdlp, ffmpeg }
}

#[cfg(windows)]
fn write_fake_runtime(bin: &std::path::Path) -> FakeRuntime {
    let ytdlp = bin.join("yt-dlp.cmd");
    let ytdlp_ps1 = bin.join("fake-ytdlp.ps1");
    std::fs::write(
        &ytdlp,
        "@powershell -NoProfile -ExecutionPolicy Bypass -File \"%~dp0fake-ytdlp.ps1\" %*\r\n",
    )
    .unwrap();
    std::fs::write(
        &ytdlp_ps1,
        r#"$out = ""
for ($i = 0; $i -lt $args.Count; $i++) {
  if ($args[$i] -eq "-o" -and ($i + 1) -lt $args.Count) {
    $out = $args[$i + 1]
    $i++
  }
}
if ($out -like "*\video\*") {
  $staging = $out -replace "\\video\\.*$", ""
  $file = Join-Path $staging "video\Fake Artist\Fake Title [vid123].mp4"
  New-Item -ItemType Directory -Force -Path (Split-Path -Parent $file) | Out-Null
  Set-Content -NoNewline -Path $file -Value "video bytes"
  Write-Output ("vid123{0}Fake Title{0}Fake Artist{0}12.5{0}{1}" -f [char]31, $file)
} elseif ($out -like "*\audio\*") {
  [Console]::Error.WriteLine("audio pass failed")
  exit 33
} else {
  [Console]::Error.WriteLine("unexpected output template: $out")
  exit 34
}
"#,
    )
    .unwrap();
    let ffmpeg = bin.join("ffmpeg.cmd");
    std::fs::write(&ffmpeg, "@exit /b 0\r\n").unwrap();
    std::fs::write(bin.join("ssh.cmd"), "@exit /b 0\r\n").unwrap();
    std::fs::write(bin.join("rsync.cmd"), "@exit /b 0\r\n").unwrap();
    FakeRuntime { ytdlp, ffmpeg }
}
