use std::path::PathBuf;

use crate::transfer_queue::{
    list_queue, prune_missing, record_failed_transfer, redact_transfer_error, retry_one,
    TransferFailureManifestInput,
};

fn test_config() -> crate::config::Config {
    crate::config::Config {
        target_path: None,
        video_target_path: None,
        allow_local_targets: false,
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

#[test]
fn record_failed_transfer_writes_manifest_with_opaque_id() {
    let dir = tempfile::tempdir().unwrap();
    let staging = dir.path().join("stage");
    std::fs::create_dir_all(staging.join("audio")).unwrap();
    let mut cfg = test_config();
    cfg.staging_dir = Some(dir.path().join("staging-root").display().to_string());
    cfg.history_path = Some(dir.path().join("downloads.jsonl").display().to_string());

    let entry = record_failed_transfer(
        &cfg,
        TransferFailureManifestInput {
            staging_path: staging.clone(),
            targets: vec![("audio".to_string(), "tootie:/music".to_string())],
            files: vec![PathBuf::from("audio/Artist/Song.mp3")],
            last_error: "rsync failed token=secret".to_string(),
        },
    )
    .unwrap();

    assert!(entry.manifest_id.starts_with("tq_"));
    assert_eq!(entry.status, "pending");
    assert!(!entry.last_error.unwrap().contains("secret"));
    assert!(entry.manifest_path.is_file());
}

#[test]
fn prune_missing_removes_only_missing_staging_entries() {
    let dir = tempfile::tempdir().unwrap();
    let staging = dir.path().join("stage");
    std::fs::create_dir_all(&staging).unwrap();
    let mut cfg = test_config();
    cfg.history_path = Some(dir.path().join("downloads.jsonl").display().to_string());

    let kept = record_failed_transfer(
        &cfg,
        TransferFailureManifestInput {
            staging_path: staging.clone(),
            targets: vec![("audio".into(), "tootie:/music".into())],
            files: vec![PathBuf::from("audio/A/B.mp3")],
            last_error: "failed".into(),
        },
    )
    .unwrap();
    std::fs::remove_dir_all(&staging).unwrap();

    let result = prune_missing(&cfg).unwrap();

    assert_eq!(result.pruned, 1);
    assert!(!kept.manifest_path.exists());
    assert!(list_queue(&cfg).unwrap().entries.is_empty());
}

#[test]
fn redact_transfer_error_masks_common_credential_shapes() {
    let redacted = redact_transfer_error(
        "failed https://user:pass@example.test/path Authorization: Bearer abc123 --token=secret password=hunter2",
    );

    assert!(!redacted.contains("user:pass"));
    assert!(!redacted.contains("abc123"));
    assert!(!redacted.contains("secret"));
    assert!(!redacted.contains("hunter2"));
    assert!(redacted.contains("https://REDACTED@example.test/path"));
    assert!(redacted.contains("Bearer REDACTED"));
    assert!(redacted.contains("--token=REDACTED"));
    assert!(redacted.contains("password=REDACTED"));
}

#[tokio::test]
async fn retry_missing_staged_kind_returns_structured_failure_and_keeps_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let staging = dir.path().join("stage");
    std::fs::create_dir_all(&staging).unwrap();
    let mut cfg = test_config();
    cfg.history_path = Some(dir.path().join("downloads.jsonl").display().to_string());

    let entry = record_failed_transfer(
        &cfg,
        TransferFailureManifestInput {
            staging_path: staging.clone(),
            targets: vec![("audio".into(), "tootie:/music".into())],
            files: vec![PathBuf::from("audio/Artist/Song.mp3")],
            last_error: "failed".into(),
        },
    )
    .unwrap();
    let manifest_path = entry.manifest_path.clone();

    let result = retry_one(&cfg, &entry.manifest_id, false).await.unwrap();

    assert_eq!(result.retried, 1);
    assert_eq!(result.completed, 0);
    assert_eq!(result.failed, 1);
    assert!(result.errors[0].contains("no staged target directories"));
    assert!(staging.exists());
    assert!(manifest_path.exists());
    assert_eq!(list_queue(&cfg).unwrap().entries[0].attempts, 1);
}
