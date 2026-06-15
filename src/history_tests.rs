use std::io::Write;
use std::path::Path;

use serde_json::{json, Value};

use super::*;
use crate::config::Config;
use crate::model::DownloadMode;
use crate::service::{DownloadFile, DownloadItem, DownloadPayload};

/// Minimal Config pointing the ledger at `path`. Only `history_path` matters
/// for these tests; everything else is a benign default.
fn config_with_history(path: &Path) -> Config {
    Config {
        remote: None,
        dest_path: None,
        video_dest_path: None,
        staging_dir: None,
        audio_format: "mp3".into(),
        ssh_opts: vec![],
        archive_dir: None,
        history_path: Some(path.to_string_lossy().into_owned()),
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

/// A representative download payload with one item, one uploader, and two files
/// of distinct kinds.
fn sample_payload(uploader: &str, transferred: bool) -> DownloadPayload {
    let file = |kind: &'static str, bytes: u64| DownloadFile {
        name: None,
        kind,
        bytes,
        title: None,
        video_id: None,
        uploader: None,
        duration: None,
    };
    DownloadPayload {
        transferred,
        transfer_error: None,
        remote: "host:/music".into(),
        dest_path: "/music".into(),
        destination: None,
        destinations: Vec::new(),
        staging_kept_at: None,
        total_files: 2,
        total_bytes: 3072,
        total_size: "3.0 KiB".into(),
        partial_items: 0,
        failed_items: 0,
        items: vec![DownloadItem {
            url: String::new(),
            status: "ok",
            title: Some("Some Track".into()),
            video_id: None,
            duration: None,
            uploader: Some(uploader.to_string()),
            is_playlist: false,
            error: None,
            files: vec![file("audio", 1024), file("thumbnail", 2048)],
        }],
        metadata_retag: None,
        plex_playlist: None,
        plex_playlist_error: None,
        history_error: None,
    }
}

#[test]
fn append_then_stats_round_trips_a_record() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("downloads.jsonl");
    let cfg = config_with_history(&path);

    append_download(&cfg, DownloadMode::Audio, &sample_payload("Artist A", true)).unwrap();

    let stats = stats_payload(&cfg, 10).unwrap();
    assert_eq!(stats["total_downloads"].as_u64(), Some(1));
    assert_eq!(stats["total_files"].as_u64(), Some(2));
    assert_eq!(stats["total_bytes"].as_u64(), Some(3072));
    assert_eq!(stats["skipped_entries"].as_u64(), Some(0));

    let recent = stats["recent"].as_array().unwrap();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0]["items"][0]["title"].as_str(), Some("Some Track"));
}

#[test]
fn round_trips_a_non_transferred_record() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("downloads.jsonl");
    let cfg = config_with_history(&path);

    append_download(
        &cfg,
        DownloadMode::Audio,
        &sample_payload("Artist B", false),
    )
    .unwrap();

    let stats = stats_payload(&cfg, 10).unwrap();
    assert_eq!(stats["total_downloads"].as_u64(), Some(1));
    let recent = stats["recent"].as_array().unwrap();
    // `transferred: false` survives the JSONL round-trip intact.
    assert_eq!(recent[0]["transferred"].as_bool(), Some(false));
}

#[test]
fn malformed_lines_are_skipped_not_panicked() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("downloads.jsonl");
    let cfg = config_with_history(&path);

    // One good record, then assorted garbage the reader must tolerate.
    append_download(&cfg, DownloadMode::Audio, &sample_payload("Artist C", true)).unwrap();
    {
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(f, "this is not json").unwrap();
        writeln!(f, "{{ \"truncated\": ").unwrap();
        writeln!(f).unwrap(); // blank line: ignored, not counted as skipped
        writeln!(f, "[1, 2, 3").unwrap();
    }

    let stats = stats_payload(&cfg, 10).unwrap();
    assert_eq!(stats["total_downloads"].as_u64(), Some(1));
    // Three malformed lines skipped; the blank line is silently ignored.
    assert_eq!(stats["skipped_entries"].as_u64(), Some(3));
}

#[test]
fn aggregation_counts_by_kind_and_uploader() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("downloads.jsonl");
    let cfg = config_with_history(&path);

    append_download(&cfg, DownloadMode::Audio, &sample_payload("Artist A", true)).unwrap();
    append_download(&cfg, DownloadMode::Audio, &sample_payload("Artist A", true)).unwrap();
    append_download(&cfg, DownloadMode::Audio, &sample_payload("Artist B", true)).unwrap();

    let stats = stats_payload(&cfg, 0).unwrap();
    assert_eq!(stats["total_downloads"].as_u64(), Some(3));
    assert_eq!(stats["total_files"].as_u64(), Some(6));
    assert_eq!(stats["total_bytes"].as_u64(), Some(9216));

    // by_kind: each download contributes one audio + one thumbnail file.
    let by_kind = &stats["by_kind"];
    assert_eq!(by_kind["audio"]["files"].as_u64(), Some(3));
    assert_eq!(by_kind["audio"]["bytes"].as_u64(), Some(3072));
    assert_eq!(by_kind["thumbnail"]["files"].as_u64(), Some(3));
    // One call per download that touched the kind.
    assert_eq!(by_kind["audio"]["calls"].as_u64(), Some(3));

    // by_uploader: Artist A appears in two downloads, Artist B in one.
    let by_uploader = &stats["by_uploader"];
    assert_eq!(by_uploader["Artist A"]["calls"].as_u64(), Some(2));
    assert_eq!(by_uploader["Artist B"]["calls"].as_u64(), Some(1));
    assert_eq!(by_uploader["Artist A"]["files"].as_u64(), Some(4));
}

#[test]
fn downloads_alias_mirrors_calls() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("downloads.jsonl");
    let cfg = config_with_history(&path);

    append_download(&cfg, DownloadMode::Audio, &sample_payload("Artist A", true)).unwrap();

    let stats = stats_payload(&cfg, 0).unwrap();
    let audio = &stats["by_kind"]["audio"];
    // The `downloads` compatibility alias is always equal to `calls`.
    assert_eq!(audio["downloads"], audio["calls"]);
    let artist = &stats["by_uploader"]["Artist A"];
    assert_eq!(artist["downloads"], artist["calls"]);
}

#[test]
fn rotation_bounds_the_ledger_and_keeps_recent_entries() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("downloads.jsonl");

    // Write well past the rotation trigger, tagging each line with an index so
    // we can confirm the *newest* entries are the ones retained.
    let total = ROTATE_TRIGGER_ENTRIES + 50;
    {
        let mut f = std::fs::File::create(&path).unwrap();
        for i in 0..total {
            let entry = json!({
                "timestamp": "2024-01-01T00:00:00Z",
                "mode": "audio",
                "seq": i,
                "total_files": 0,
                "total_bytes": 0,
                "items": [],
            });
            writeln!(f, "{}", serde_json::to_string(&entry).unwrap()).unwrap();
        }
    }
    assert!(total > ROTATE_TRIGGER_ENTRIES);

    // Trigger rotation through the public append path (the cap is enforced as a
    // best-effort side effect of appending).
    let cfg = config_with_history(&path);
    append_download(&cfg, DownloadMode::Audio, &sample_payload("Artist Z", true)).unwrap();

    // The file is now bounded to the cap (plus the line we just appended).
    let contents = std::fs::read_to_string(&path).unwrap();
    let kept: Vec<&str> = contents.lines().collect();
    assert!(
        kept.len() <= MAX_HISTORY_ENTRIES + 1,
        "ledger not bounded: {} lines",
        kept.len()
    );

    // The oldest entries were dropped: seq 0 must be gone, and the appended
    // record (the newest) must be present.
    let first: Value = serde_json::from_str(kept.first().unwrap()).unwrap();
    assert!(
        first["seq"].as_u64().unwrap() > 0,
        "oldest entries were not trimmed"
    );
    assert_eq!(
        kept.iter()
            .filter(|l| l.contains("\"uploader\":\"Artist Z\""))
            .count(),
        1,
        "newest appended record was lost"
    );

    // No temp file is left behind after a successful rotation.
    assert!(!path.with_extension("jsonl.tmp").exists());

    // Stats still parse the trimmed ledger cleanly.
    let stats = stats_payload(&cfg, 5).unwrap();
    assert_eq!(stats["skipped_entries"].as_u64(), Some(0));
}
