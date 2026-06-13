use super::*;

struct FakeLookup {
    seen: Vec<Fingerprint>,
    score: f64,
}

impl AcoustIdLookup for FakeLookup {
    fn lookup(&mut self, fingerprint: &Fingerprint) -> anyhow::Result<Vec<IdentifyCandidate>> {
        self.seen.push(fingerprint.clone());
        Ok(vec![IdentifyCandidate {
            acoustid_id: "acoustid-1".into(),
            score: self.score,
            recording_id: Some("recording-1".into()),
            title: Some("True Love Waits".into()),
            artists: vec!["Goose".into()],
            release_group: Some("Viva El Gonzo".into()),
            release_group_type: Some("Live".into()),
        }])
    }
}

struct FakeMusicBrainz {
    seen: Vec<String>,
}

impl MusicBrainzLookup for FakeMusicBrainz {
    fn lookup_recording(&mut self, recording_id: &str, score: f64) -> anyhow::Result<RetagPreview> {
        self.seen.push(recording_id.to_string());
        Ok(RetagPreview {
            confidence: score,
            recording_id: recording_id.to_string(),
            recording_title: "True Love Waits".into(),
            artist: "Goose".into(),
            artists: vec!["Goose".into()],
            release_id: Some("release-1".into()),
            release_title: Some("Viva El Gonzo".into()),
            release_group_id: Some("rg-1".into()),
            release_group_title: Some("Viva El Gonzo".into()),
            release_type: Some("Live".into()),
            release_date: Some("2025-04-25".into()),
            track_number: Some("7".into()),
            musicbrainz_url: "https://musicbrainz.org/recording/recording-1".into(),
        })
    }
}

#[tokio::test]
async fn identify_file_runs_fpcalc_and_looks_up_candidates() {
    let dir = tempfile::tempdir().unwrap();
    let audio = dir.path().join("song.mp3");
    std::fs::write(&audio, b"fake audio").unwrap();
    let fpcalc = write_fake_fpcalc(dir.path());
    let mut lookup = FakeLookup {
        seen: Vec::new(),
        score: 0.91,
    };

    let result = identify_file_with_client(
        &fpcalc,
        &audio,
        Some(std::time::Duration::from_secs(5)),
        &mut lookup,
        None,
        false,
    )
    .await;

    assert_eq!(result.path, audio.display().to_string());
    assert!(result.error.is_none(), "{:?}", result.error);
    assert_eq!(result.duration, Some(185));
    assert_eq!(result.candidates.len(), 1);
    assert_eq!(lookup.seen.len(), 1);
    assert_eq!(lookup.seen[0].duration, 185);
    assert_eq!(lookup.seen[0].fingerprint, "AQABz0qUkZ");
}

#[tokio::test]
async fn identify_file_accepts_fpcalc_fingerprint_with_warning_exit() {
    let dir = tempfile::tempdir().unwrap();
    let audio = dir.path().join("song.mp3");
    std::fs::write(&audio, b"fake audio").unwrap();
    let fpcalc = write_warning_fpcalc(dir.path());
    let mut lookup = FakeLookup {
        seen: Vec::new(),
        score: 0.91,
    };

    let result = identify_file_with_client(
        &fpcalc,
        &audio,
        Some(std::time::Duration::from_secs(5)),
        &mut lookup,
        None,
        false,
    )
    .await;

    assert!(result.error.is_none(), "{:?}", result.error);
    assert_eq!(result.duration, Some(185));
    assert_eq!(result.candidates.len(), 1);
    assert_eq!(lookup.seen[0].fingerprint, "AQABz0qUkZ");
}

#[tokio::test]
async fn identify_file_enriches_high_confidence_candidate_with_retag_preview() {
    let dir = tempfile::tempdir().unwrap();
    let audio = dir.path().join("song.mp3");
    std::fs::write(&audio, b"fake audio").unwrap();
    let fpcalc = write_fake_fpcalc(dir.path());
    let mut acoustid = FakeLookup {
        seen: Vec::new(),
        score: 0.98,
    };
    let mut musicbrainz = FakeMusicBrainz { seen: Vec::new() };

    let result = identify_file_with_client(
        &fpcalc,
        &audio,
        Some(std::time::Duration::from_secs(5)),
        &mut acoustid,
        Some(&mut musicbrainz),
        false,
    )
    .await;

    let preview = result.retag_preview.expect("retag preview");
    assert_eq!(musicbrainz.seen, vec!["recording-1"]);
    assert_eq!(preview.recording_title, "True Love Waits");
    assert_eq!(preview.artist, "Goose");
    assert_eq!(preview.release_title.as_deref(), Some("Viva El Gonzo"));
    assert_eq!(preview.release_date.as_deref(), Some("2025-04-25"));
    assert_eq!(preview.track_number.as_deref(), Some("7"));
    assert!(result.retag_preview_error.is_none());
}

#[test]
fn parse_fpcalc_output_extracts_duration_and_fingerprint() {
    let output = b"FILE=/tmp/song.mp3\nDURATION=185\nFINGERPRINT=AQABz0qUkZ\n";

    let fingerprint = parse_fpcalc_output(output).expect("fingerprint");

    assert_eq!(fingerprint.duration, 185);
    assert_eq!(fingerprint.fingerprint, "AQABz0qUkZ");
}

#[test]
fn parse_acoustid_lookup_extracts_recording_candidates() {
    let json = br#"{
      "status": "ok",
      "results": [
        {
          "id": "acoustid-1",
          "score": 0.91,
          "recordings": [
            {
              "id": "recording-1",
              "title": "True Love Waits",
              "duration": 312000,
              "artists": [{"id":"artist-1","name":"Goose"}],
              "releasegroups": [{"id":"rg-1","title":"Viva El Gonzo","type":"Live"}]
            }
          ]
        }
      ]
    }"#;

    let candidates = parse_acoustid_lookup(json).expect("lookup candidates");

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].acoustid_id, "acoustid-1");
    assert_eq!(candidates[0].score, 0.91);
    assert_eq!(candidates[0].recording_id.as_deref(), Some("recording-1"));
    assert_eq!(candidates[0].title.as_deref(), Some("True Love Waits"));
    assert_eq!(candidates[0].artists, vec!["Goose"]);
    assert_eq!(
        candidates[0].release_group.as_deref(),
        Some("Viva El Gonzo")
    );
    assert_eq!(candidates[0].release_group_type.as_deref(), Some("Live"));
}

#[cfg(unix)]
fn write_fake_fpcalc(dir: &std::path::Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let fpcalc = dir.join("fpcalc");
    std::fs::write(
        &fpcalc,
        "#!/bin/sh\nprintf 'FILE=%s\\nDURATION=185\\nFINGERPRINT=AQABz0qUkZ\\n' \"$1\"\n",
    )
    .unwrap();
    let mut perms = std::fs::metadata(&fpcalc).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&fpcalc, perms).unwrap();
    fpcalc
}

#[cfg(unix)]
fn write_warning_fpcalc(dir: &std::path::Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let fpcalc = dir.join("fpcalc-warning");
    std::fs::write(
        &fpcalc,
        "#!/bin/sh\nprintf 'ERROR: Error decoding audio frame (End of file)\\n' >&2\nprintf 'FILE=%s\\nDURATION=185\\nFINGERPRINT=AQABz0qUkZ\\n' \"$1\"\nexit 1\n",
    )
    .unwrap();
    let mut perms = std::fs::metadata(&fpcalc).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&fpcalc, perms).unwrap();
    fpcalc
}

#[cfg(windows)]
fn write_fake_fpcalc(dir: &std::path::Path) -> std::path::PathBuf {
    let fpcalc = dir.join("fpcalc.cmd");
    std::fs::write(
        &fpcalc,
        "@echo FILE=%1\r\n@echo DURATION=185\r\n@echo FINGERPRINT=AQABz0qUkZ\r\n",
    )
    .unwrap();
    fpcalc
}

#[cfg(windows)]
fn write_warning_fpcalc(dir: &std::path::Path) -> std::path::PathBuf {
    let fpcalc = dir.join("fpcalc-warning.cmd");
    std::fs::write(
        &fpcalc,
        "@echo ERROR: Error decoding audio frame (End of file) 1>&2\r\n@echo FILE=%1\r\n@echo DURATION=185\r\n@echo FINGERPRINT=AQABz0qUkZ\r\n@exit /b 1\r\n",
    )
    .unwrap();
    fpcalc
}
