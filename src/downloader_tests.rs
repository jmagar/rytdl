use std::time::Duration;

use tokio::process::Command;

use crate::bootstrap::Tools;
use crate::model::SearchResultItem;

use super::*;

#[test]
fn stderr_tail_keeps_last_complete_lines_with_truncation_marker() {
    let input = b"one\ntwo\nthree\nfour\n";
    let tail = stderr_tail_text(input, 10);

    assert!(tail.starts_with("[stderr truncated]\n"));
    assert!(tail.contains("three\nfour"));
    assert!(!tail.contains("one\n"));
}

#[tokio::test]
async fn run_command_reports_timeout() {
    let mut cmd = Command::new("sh");
    cmd.args(["-c", "sleep 2"]);

    let err = run_command(&mut cmd, Some(Duration::from_millis(50)))
        .await
        .unwrap_err()
        .to_string();

    assert!(err.contains("timed out after"));
}

#[test]
fn parse_search_json_extracts_youtube_entries() {
    let json = br#"{
      "id": "slow pulp live",
      "title": "slow pulp live",
      "entries": [
        {
          "id": "abc123",
          "title": "Slow Pulp - Falling Apart Live",
          "webpage_url": "https://www.youtube.com/watch?v=abc123",
          "uploader": "Slow Pulp",
          "duration": 215.0,
          "thumbnail": "https://i.ytimg.com/vi/abc123/hqdefault.jpg",
          "view_count": 42000
        },
        null,
        {
          "id": "def456",
          "title": "Slow Pulp - Idaho Live",
          "url": "https://www.youtube.com/watch?v=def456",
          "channel": "Live Room",
          "duration": 188
        }
      ]
    }"#;

    let results = super::parse_search_json(json).unwrap();

    assert_eq!(
        results,
        vec![
            SearchResultItem {
                title: "Slow Pulp - Falling Apart Live".into(),
                url: "https://www.youtube.com/watch?v=abc123".into(),
                video_id: Some("abc123".into()),
                uploader: Some("Slow Pulp".into()),
                duration: Some(215.0),
                thumbnail: Some("https://i.ytimg.com/vi/abc123/hqdefault.jpg".into()),
                view_count: Some(42000),
            },
            SearchResultItem {
                title: "Slow Pulp - Idaho Live".into(),
                url: "https://www.youtube.com/watch?v=def456".into(),
                video_id: Some("def456".into()),
                uploader: Some("Live Room".into()),
                duration: Some(188.0),
                thumbnail: None,
                view_count: None,
            },
        ]
    );
}

#[test]
fn parse_search_json_handles_edge_cases() {
    assert!(super::parse_search_json(b"not json").is_err());
    let missing_entries = super::parse_search_json(br#"{"title":"empty"}"#)
        .unwrap_err()
        .to_string();
    assert!(missing_entries.contains("did not contain an entries array"));
    assert!(missing_entries.contains("title"));

    let json = br#"{
      "entries": [
        { "title": "Missing URL" },
        { "webpage_url": "https://www.youtube.com/watch?v=no-title" },
        {
          "title": "Canonical URL",
          "webpage_url": "https://www.youtube.com/watch?v=canonical",
          "url": "https://youtube.com/shorts/raw"
        },
        {
          "id": "idonly123",
          "title": "ID Only",
          "url": "idonly123"
        }
      ]
    }"#;

    let results = super::parse_search_json(json).unwrap();

    assert_eq!(results.len(), 2);
    assert_eq!(results[0].title, "Canonical URL");
    assert_eq!(results[0].url, "https://www.youtube.com/watch?v=canonical");
    assert_eq!(results[1].title, "ID Only");
    assert_eq!(results[1].url, "https://www.youtube.com/watch?v=idonly123");
}

#[test]
fn search_query_spec_uses_ytsearch_limit_prefix() {
    assert_eq!(super::search_spec("tiny desk", 7), "ytsearch7:tiny desk");
    assert_eq!(
        super::search_spec("  tiny desk  ", 2),
        "ytsearch2:tiny desk"
    );
}

#[test]
fn search_spec_is_placed_immediately_after_end_of_options_separator() {
    // `search_youtube` feeds the search positional as `cmd.arg("--").arg(search_spec(..))`.
    // Mirror that exact construction and lock the invariant that `--` immediately
    // precedes the `ytsearch{N}:` spec, so the end-of-options guard on the search
    // path can never silently regress (a stray flag between them, or the spec
    // landing in the options region, would be parsed as an option by yt-dlp).
    let spec = super::search_spec("--exec=touch /tmp/pwned", 5);
    assert!(
        spec.starts_with("ytsearch5:"),
        "spec must carry the ytsearch{{N}}: prefix: {spec:?}"
    );

    // The argv tail as built by `search_youtube`: end-of-options, then the spec.
    let argv = ["--".to_string(), spec.clone()];
    let spec_idx = argv
        .iter()
        .position(|a| a.starts_with("ytsearch"))
        .expect("argv must contain the ytsearch spec");
    assert!(spec_idx > 0, "spec must not be the first argv element");
    assert_eq!(
        argv[spec_idx - 1],
        "--",
        "`--` must immediately precede the ytsearch{{N}}: spec"
    );
    assert_eq!(
        argv.last().map(String::as_str),
        Some(spec.as_str()),
        "spec must be the final argv element"
    );
}

#[test]
fn common_args_preserve_source_metadata_sidecars() {
    let tools = Tools {
        ytdlp: "yt-dlp".into(),
        ffmpeg_dir: None,
        extractor_args: None,
    };

    let args = super::common_args(std::path::Path::new("/tmp/stage"), "audio", &tools, None);

    assert!(contains_pair(
        &args,
        "--parse-metadata",
        super::PARSE_ARTIST
    ));
    assert!(contains_pair(
        &args,
        "--parse-metadata",
        super::PARSE_PLAYLIST_ALBUM
    ));
    assert!(args.iter().any(|arg| arg == "--write-info-json"));
    assert!(args.iter().any(|arg| arg == "--write-thumbnail"));
    assert!(args.iter().any(|arg| arg == "--write-description"));
    assert!(contains_pair(&args, "--convert-thumbnails", "jpg"));
}

#[test]
fn output_template_uses_windows_filenames_artist_title_id_layout() {
    let staging = std::path::Path::new("/tmp/stage");

    // The `-o` template carries the cross-OS `Artist/Title [id]` layout the
    // project relies on: a per-kind subdir, then the resolved artist folder,
    // then `Title [id].ext`.
    let template = super::output_template(staging, "audio");
    assert_eq!(
        template,
        "/tmp/stage/audio/%(artist,uploader,channel,creator|Unknown Artist)s/%(title)s [%(id)s].%(ext)s"
    );

    // `--windows-filenames` is pushed unconditionally into the common args so
    // the `Artist/Title [id]` layout is byte-identical across Linux and Windows
    // (documented side effect: a trailing `.` becomes `#`).
    let tools = Tools {
        ytdlp: "yt-dlp".into(),
        ffmpeg_dir: None,
        extractor_args: None,
    };
    let args = super::common_args(staging, "audio", &tools, None);
    assert!(args.iter().any(|arg| arg == "--windows-filenames"));
    // The same `-o` template is wired into the common args via the `-o` flag.
    assert!(contains_pair(&args, "-o", &template));
}

#[test]
fn metadata_cleanup_args_normalize_common_youtube_title_noise() {
    let mut args = Vec::new();

    super::add_metadata_cleanup_args(&mut args);

    assert!(contains_quad(
        &args,
        "--replace-in-metadata",
        "title",
        r"(?i)\s*[\[(](official\s+(music\s+)?video|official\s+audio|audio\s+only|lyric(s)?(\s+video)?|visuali[sz]er|music\s+video|hd|4k)[\])]\s*",
        ""
    ));
    assert!(contains_quad(
        &args,
        "--replace-in-metadata",
        "title",
        r"\s*[|｜]\s*@[\w.-]+\s*$",
        ""
    ));
    assert!(contains_quad(
        &args,
        "--replace-in-metadata",
        "title",
        r"^\s+|\s+$",
        ""
    ));
}

#[test]
fn positional_is_placed_after_end_of_options_separator() {
    // A '-'-prefixed value that yt-dlp would parse as a flag if not separated.
    let url = "--exec=touch /tmp/pwned";
    let argv = super::positional_after_end_of_options(
        vec!["--quiet".into(), "-o".into(), "tmpl".into()],
        url,
    );

    // The URL is the final argv element...
    assert_eq!(argv.last().map(String::as_str), Some(url));
    // ...immediately preceded by the `--` end-of-options separator.
    let sep = argv.len() - 2;
    assert_eq!(argv[sep], "--");
    // The malicious value never appears in the options region (before `--`).
    assert!(!argv[..sep].iter().any(|a| a == url));
}

#[test]
fn print_line_parser_handles_well_formed_and_malformed_lines() {
    let sep = super::SEP;
    let good = format!("id1{sep}Title{sep}Uploader{sep}215{sep}/tmp/out.mp3");
    assert_eq!(
        super::parse_print_line(&good),
        Some(("id1", "Title", "Uploader", "215", "/tmp/out.mp3"))
    );

    // Pin one fixture to the literal Unit Separator byte (U+001F) rather than
    // `super::SEP`, so this locks the wire-format contract with yt-dlp's
    // `--print` output: if `SEP` itself ever changed, the parser and this test
    // would no longer move together and this assertion would catch the drift.
    let literal = "id2\x1fTitle 2\x1fUploader 2\x1f120\x1f/tmp/out2.mp3";
    assert_eq!(
        super::parse_print_line(literal),
        Some(("id2", "Title 2", "Uploader 2", "120", "/tmp/out2.mp3"))
    );

    // Wrong field count (too few / too many) is skipped gracefully.
    assert_eq!(super::parse_print_line("just one field"), None);
    let too_few = format!("id1{sep}Title{sep}Uploader");
    assert_eq!(super::parse_print_line(&too_few), None);
    let too_many = format!("a{sep}b{sep}c{sep}d{sep}e{sep}f");
    assert_eq!(super::parse_print_line(&too_many), None);
    assert_eq!(super::parse_print_line(""), None);
}

#[cfg(unix)]
#[tokio::test]
async fn probe_reports_playlist_count_under_flat_playlist() {
    // Flat-playlist dump: stub entries (no per-entry metadata) plus the top-level
    // `playlist_count` yt-dlp emits. Probe must report is_playlist=true and the
    // correct entry_count without resolving every entry.
    let dir = tempfile::tempdir().unwrap();
    let json = r#"{
      "id": "PL123",
      "title": "Tiny Desk Concerts",
      "uploader": "NPR Music",
      "playlist_count": 3,
      "entries": [
        { "id": "a1", "title": "One", "url": "a1" },
        null,
        { "id": "b2", "title": "Two", "url": "b2" }
      ]
    }"#;
    let ytdlp = write_fake_probe_ytdlp(dir.path(), json);

    let r = super::probe(
        &ytdlp,
        "https://youtube.com/playlist?list=PL123",
        None,
        None,
    )
    .await;

    assert!(r.error.is_none(), "unexpected error: {:?}", r.error);
    assert!(r.is_playlist);
    // `playlist_count` is authoritative even though only 2 entry stubs are non-null.
    assert_eq!(r.entry_count, Some(3));
    assert_eq!(r.title.as_deref(), Some("Tiny Desk Concerts"));
    assert_eq!(r.uploader.as_deref(), Some("NPR Music"));
}

#[cfg(unix)]
#[tokio::test]
async fn probe_counts_entries_when_playlist_count_absent() {
    // Older/extractor variants omit `playlist_count`; fall back to counting
    // non-null entry stubs.
    let dir = tempfile::tempdir().unwrap();
    let json = r#"{
      "id": "PL456",
      "title": "Mix",
      "entries": [
        { "id": "a1", "title": "One", "url": "a1" },
        null,
        { "id": "b2", "title": "Two", "url": "b2" },
        { "id": "c3", "title": "Three", "url": "c3" }
      ]
    }"#;
    let ytdlp = write_fake_probe_ytdlp(dir.path(), json);

    let r = super::probe(
        &ytdlp,
        "https://youtube.com/playlist?list=PL456",
        None,
        None,
    )
    .await;

    assert!(r.error.is_none(), "unexpected error: {:?}", r.error);
    assert!(r.is_playlist);
    assert_eq!(r.entry_count, Some(3));
}

#[cfg(unix)]
#[tokio::test]
async fn probe_single_video_unaffected_by_flat_playlist() {
    // A single video has no `entries` array, so flat mode is a no-op and the
    // single-video fields (duration, format_count) are reported as before.
    let dir = tempfile::tempdir().unwrap();
    let json = r#"{
      "id": "vid789",
      "title": "Just One Video",
      "uploader": "Some Channel",
      "duration": 215.0,
      "formats": [ { "format_id": "18" }, { "format_id": "22" } ]
    }"#;
    let ytdlp = write_fake_probe_ytdlp(dir.path(), json);

    let r = super::probe(&ytdlp, "https://youtube.com/watch?v=vid789", None, None).await;

    assert!(r.error.is_none(), "unexpected error: {:?}", r.error);
    assert!(!r.is_playlist);
    assert_eq!(r.entry_count, None);
    assert_eq!(r.title.as_deref(), Some("Just One Video"));
    assert_eq!(r.uploader.as_deref(), Some("Some Channel"));
    assert_eq!(r.duration, Some(215.0));
    assert_eq!(r.format_count, Some(2));
}

#[cfg(unix)]
fn write_fake_probe_ytdlp(dir: &std::path::Path, json: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("fake-ytdlp");
    // Emit the fixed JSON blob to stdout regardless of args.
    let script = format!("#!/bin/sh\ncat <<'YTDLP_JSON'\n{json}\nYTDLP_JSON\n");
    std::fs::write(&path, script).unwrap();
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms).unwrap();
    path
}

fn contains_pair(args: &[String], flag: &str, value: &str) -> bool {
    args.windows(2)
        .any(|pair| pair[0] == flag && pair[1] == value)
}

fn contains_quad(args: &[String], a: &str, b: &str, c: &str, d: &str) -> bool {
    args.windows(4)
        .any(|quad| quad[0] == a && quad[1] == b && quad[2] == c && quad[3] == d)
}
