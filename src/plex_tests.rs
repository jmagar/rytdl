use super::*;
use crate::downloader::{ItemResult, MediaFile};
use serde_json::json;

#[derive(Default)]
struct FakePlex {
    gets: Vec<(String, Vec<(String, String)>)>,
    posts: Vec<(String, Vec<(String, String)>)>,
    puts: Vec<(String, Vec<(String, String)>)>,
}

impl PlexTransport for FakePlex {
    fn get(&mut self, path: &str, params: &[(&str, &str)]) -> Result<Value> {
        self.gets.push((
            path.to_string(),
            params
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect(),
        ));
        match path {
            "/identity" => Ok(json!({
                "MediaContainer": { "machineIdentifier": "machine-1" }
            })),
            "/playlists" => Ok(json!({
                "MediaContainer": {
                    "Metadata": [
                        { "ratingKey": "99", "title": "Downloads" }
                    ]
                }
            })),
            "/playlists/99/items" => Ok(json!({
                "MediaContainer": {
                    "Metadata": [
                        { "ratingKey": "111", "title": "Already There" }
                    ]
                }
            })),
            "/search" => {
                let query = params
                    .iter()
                    .find(|(key, _)| *key == "query")
                    .map(|(_, value)| *value)
                    .unwrap_or("");
                if query == "Already There" {
                    Ok(json!({
                        "MediaContainer": {
                            "Metadata": [
                                { "ratingKey": "111", "type": "track", "title": "Already There", "grandparentTitle": "Artist A" }
                            ]
                        }
                    }))
                } else if query == "New Song" {
                    Ok(json!({
                        "MediaContainer": {
                            "Metadata": [
                                { "ratingKey": "222", "type": "track", "title": "New Song", "grandparentTitle": "Artist B" }
                            ]
                        }
                    }))
                } else {
                    Ok(json!({ "MediaContainer": { "Metadata": [] } }))
                }
            }
            _ => bail!("unexpected GET {path}"),
        }
    }

    fn post(&mut self, path: &str, params: &[(&str, &str)]) -> Result<Value> {
        self.posts.push((
            path.to_string(),
            params
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect(),
        ));
        Ok(json!({
            "MediaContainer": {
                "Metadata": [
                    { "ratingKey": "100", "title": "Downloads" }
                ]
            }
        }))
    }

    fn put(&mut self, path: &str, params: &[(&str, &str)]) -> Result<()> {
        self.puts.push((
            path.to_string(),
            params
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect(),
        ));
        Ok(())
    }
}

#[test]
fn add_downloaded_audio_skips_existing_and_adds_missing_playlist_items() {
    let mut plex = FakePlex::default();
    let results = vec![ItemResult {
        files: vec![
            audio_file("Already There", "Artist A"),
            audio_file("New Song", "Artist B"),
            video_file("Ignored Video", "Artist C"),
        ],
        ..Default::default()
    }];

    let update =
        add_downloaded_audio_with_transport(&mut plex, "Downloads", &results).expect("plex update");

    assert_eq!(update.playlist, "Downloads");
    assert_eq!(update.playlist_id.as_deref(), Some("99"));
    assert_eq!(update.matched, 2);
    assert_eq!(update.added, 1);
    assert_eq!(update.already_present, 1);
    assert!(update.missing.is_empty());
    assert_eq!(plex.posts.len(), 0);
    assert_eq!(plex.puts.len(), 1);
    assert_eq!(plex.puts[0].0, "/playlists/99/items");
    assert_eq!(
        plex.puts[0].1[0].1,
        "server://machine-1/com.plexapp.plugins.library/library/metadata/222"
    );
}

#[test]
fn add_downloaded_audio_creates_playlist_with_first_matched_track() {
    let mut plex = CreatePlaylistPlex::default();
    let results = vec![ItemResult {
        files: vec![audio_file("New Song", "Artist B")],
        ..Default::default()
    }];

    let update = add_downloaded_audio_with_transport(&mut plex, "Fresh List", &results)
        .expect("plex update");

    assert_eq!(update.playlist_id.as_deref(), Some("100"));
    assert_eq!(update.matched, 1);
    assert_eq!(update.added, 1);
    assert_eq!(plex.posts.len(), 1);
    assert_eq!(plex.posts[0].0, "/playlists");
    assert!(plex.puts.is_empty());
}

#[test]
fn add_downloaded_audio_without_audio_files_does_not_require_plex_config() {
    let cfg = Config {
        remote: None,
        dest_path: None,
        video_dest_path: None,
        staging_dir: None,
        audio_format: "mp3".into(),
        ssh_opts: Vec::new(),
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
    };
    let results = vec![ItemResult {
        files: vec![video_file("Ignored Video", "Artist C")],
        ..Default::default()
    }];

    let update = add_downloaded_audio(&cfg, "Downloads", &results).expect("empty update");

    assert_eq!(update.playlist, "Downloads");
    assert_eq!(update.matched, 0);
    assert_eq!(update.added, 0);
    assert_eq!(update.already_present, 0);
}

#[derive(Default)]
struct CreatePlaylistPlex {
    posts: Vec<(String, Vec<(String, String)>)>,
    puts: Vec<(String, Vec<(String, String)>)>,
}

impl PlexTransport for CreatePlaylistPlex {
    fn get(&mut self, path: &str, params: &[(&str, &str)]) -> Result<Value> {
        match path {
            "/identity" => Ok(json!({
                "MediaContainer": { "machineIdentifier": "machine-1" }
            })),
            "/playlists" => Ok(json!({ "MediaContainer": { "Metadata": [] } })),
            "/search" => {
                let query = params
                    .iter()
                    .find(|(key, _)| *key == "query")
                    .map(|(_, value)| *value)
                    .unwrap_or("");
                assert_eq!(query, "New Song");
                Ok(json!({
                    "MediaContainer": {
                        "Metadata": [
                            { "ratingKey": "222", "type": "track", "title": "New Song", "grandparentTitle": "Artist B" }
                        ]
                    }
                }))
            }
            _ => bail!("unexpected GET {path}"),
        }
    }

    fn post(&mut self, path: &str, params: &[(&str, &str)]) -> Result<Value> {
        self.posts.push((
            path.to_string(),
            params
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect(),
        ));
        Ok(json!({
            "MediaContainer": {
                "Metadata": [
                    { "ratingKey": "100", "title": "Fresh List" }
                ]
            }
        }))
    }

    fn put(&mut self, path: &str, params: &[(&str, &str)]) -> Result<()> {
        self.puts.push((
            path.to_string(),
            params
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect(),
        ));
        Ok(())
    }
}

fn audio_file(title: &str, uploader: &str) -> MediaFile {
    media_file("audio", title, uploader)
}

fn video_file(title: &str, uploader: &str) -> MediaFile {
    media_file("video", title, uploader)
}

fn media_file(kind: &'static str, title: &str, uploader: &str) -> MediaFile {
    MediaFile {
        path: format!("{title}.mp3").into(),
        kind,
        size: 10,
        title: Some(title.to_string()),
        video_id: Some(format!("{title}-id")),
        uploader: Some(uploader.to_string()),
        duration: Some(120.0),
    }
}
