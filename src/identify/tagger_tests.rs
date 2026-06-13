use lofty::tag::{Accessor, ItemKey, TagType};
use std::process::Command;

use super::*;

fn preview() -> RetagPreview {
    RetagPreview {
        confidence: 0.98,
        recording_id: "recording-1".into(),
        recording_title: "Harry Hood".into(),
        artist: "Phish".into(),
        artists: vec!["Phish".into()],
        release_id: Some("release-1".into()),
        release_title: Some("2024-08-29: Dick's Sporting Goods Park".into()),
        release_group_id: Some("group-1".into()),
        release_group_title: Some("2024-08-29: Dick's Sporting Goods Park".into()),
        release_type: Some("Album, Live".into()),
        release_date: Some("2024-08-29".into()),
        track_number: Some("3".into()),
        musicbrainz_url: "https://musicbrainz.org/recording/recording-1".into(),
    }
}

#[test]
fn apply_preview_to_tag_writes_common_and_musicbrainz_fields() {
    let mut tag = Tag::new(TagType::VorbisComments);

    let fields = apply_preview_to_tag(&mut tag, &preview());

    assert_eq!(tag.artist().as_deref(), Some("Phish"));
    assert_eq!(tag.title().as_deref(), Some("Harry Hood"));
    assert_eq!(
        tag.album().as_deref(),
        Some("2024-08-29: Dick's Sporting Goods Park")
    );
    assert_eq!(tag.get_string(ItemKey::AlbumArtist), Some("Phish"));
    assert_eq!(tag.get_string(ItemKey::RecordingDate), Some("2024-08-29"));
    assert_eq!(tag.get_string(ItemKey::ReleaseDate), Some("2024-08-29"));
    assert_eq!(tag.get_string(ItemKey::TrackNumber), Some("3"));
    assert_eq!(
        tag.get_string(ItemKey::MusicBrainzRecordingId),
        Some("recording-1")
    );
    assert_eq!(
        tag.get_string(ItemKey::MusicBrainzReleaseId),
        Some("release-1")
    );
    assert_eq!(
        tag.get_string(ItemKey::MusicBrainzReleaseGroupId),
        Some("group-1")
    );
    assert_eq!(
        tag.get_string(ItemKey::MusicBrainzReleaseType),
        Some("Album, Live")
    );
    assert!(fields.contains(&"musicbrainz_recording_id".to_string()));
}

#[test]
fn apply_preview_to_tag_skips_missing_optional_release_fields() {
    let mut preview = preview();
    preview.release_id = None;
    preview.release_group_id = None;
    preview.release_type = None;
    let mut tag = Tag::new(TagType::VorbisComments);

    let fields = apply_preview_to_tag(&mut tag, &preview);

    assert_eq!(
        tag.get_string(ItemKey::MusicBrainzRecordingId),
        Some("recording-1")
    );
    assert!(tag.get_string(ItemKey::MusicBrainzReleaseId).is_none());
    assert!(tag.get_string(ItemKey::MusicBrainzReleaseGroupId).is_none());
    assert!(tag.get_string(ItemKey::MusicBrainzReleaseType).is_none());
    assert!(!fields.contains(&"musicbrainz_release_id".to_string()));
}

#[test]
fn write_retag_preview_reports_invalid_audio_read_errors() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("not-a-song.mp3");
    std::fs::write(&path, b"not audio").unwrap();

    let err = write_retag_preview(&path, &preview())
        .unwrap_err()
        .to_string();

    assert!(err.contains("read audio tags"));
}

#[test]
fn write_retag_preview_persists_tags_to_generated_flac_when_ffmpeg_exists() {
    if which::which("ffmpeg").is_err() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tone.flac");
    let status = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=0.1",
            "-c:a",
            "flac",
            "-f",
            "flac",
            "-y",
        ])
        .arg(&path)
        .status()
        .unwrap();
    if !status.success() || !path.is_file() {
        return;
    }

    let result = write_retag_preview(&path, &preview()).unwrap();
    let tagged_file = lofty::read_from_path(&path).unwrap();
    let tag = tagged_file.primary_tag().expect("primary tag");

    assert!(result.written);
    assert_eq!(tag.artist().as_deref(), Some("Phish"));
    assert_eq!(tag.title().as_deref(), Some("Harry Hood"));
    assert_eq!(
        tag.get_string(ItemKey::MusicBrainzReleaseId),
        Some("release-1")
    );
}
