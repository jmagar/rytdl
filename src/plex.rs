//! Optional Plex playlist integration for downloaded audio tracks.

use std::collections::BTreeSet;

use anyhow::{bail, Context, Result};
use serde::Serialize;
use serde_json::Value;
use url::Url;

use crate::config::Config;
use crate::downloader::ItemResult;

const TRACK_TYPE: &str = "10";

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PlexPlaylistUpdate {
    pub playlist: String,
    pub playlist_id: Option<String>,
    pub matched: usize,
    pub added: usize,
    pub already_present: usize,
    pub missing: Vec<PlexMissingTrack>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PlexMissingTrack {
    pub title: String,
    pub uploader: Option<String>,
}

#[derive(Debug, Clone)]
struct TrackCandidate {
    title: String,
    uploader: Option<String>,
}

pub fn add_downloaded_audio(
    cfg: &Config,
    playlist: &str,
    results: &[ItemResult],
) -> Result<PlexPlaylistUpdate> {
    let tracks = audio_tracks(results);
    if tracks.is_empty() {
        return Ok(PlexPlaylistUpdate {
            playlist: playlist.to_string(),
            playlist_id: None,
            matched: 0,
            added: 0,
            already_present: 0,
            missing: Vec::new(),
            errors: Vec::new(),
        });
    }

    let base_url = cfg
        .plex_url
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .context("YTDLP_PLEX_URL is required when plex_playlist is set")?;
    let token = cfg
        .plex_token
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .context("YTDLP_PLEX_TOKEN is required when plex_playlist is set")?;
    let mut transport = UreqPlexTransport {
        base_url: base_url.to_string(),
        token: token.to_string(),
    };
    add_audio_tracks_with_transport(&mut transport, playlist, tracks)
}

#[cfg(test)]
fn add_downloaded_audio_with_transport(
    transport: &mut impl PlexTransport,
    playlist: &str,
    results: &[ItemResult],
) -> Result<PlexPlaylistUpdate> {
    let tracks = audio_tracks(results);
    add_audio_tracks_with_transport(transport, playlist, tracks)
}

fn add_audio_tracks_with_transport(
    transport: &mut impl PlexTransport,
    playlist: &str,
    tracks: Vec<TrackCandidate>,
) -> Result<PlexPlaylistUpdate> {
    let mut update = PlexPlaylistUpdate {
        playlist: playlist.to_string(),
        playlist_id: None,
        matched: 0,
        added: 0,
        already_present: 0,
        missing: Vec::new(),
        errors: Vec::new(),
    };

    if tracks.is_empty() {
        return Ok(update);
    }

    let machine_id = machine_identifier(transport)?;
    let mut playlist_id = find_playlist_id(transport, playlist)?;
    let mut existing = match playlist_id.as_deref() {
        Some(id) => playlist_item_keys(transport, id)?,
        None => BTreeSet::new(),
    };

    for track in tracks {
        let Some(rating_key) = find_track_rating_key(transport, &track)? else {
            update.missing.push(PlexMissingTrack {
                title: track.title,
                uploader: track.uploader,
            });
            continue;
        };
        update.matched += 1;

        if playlist_id.is_none() {
            let id = create_playlist(transport, playlist, &machine_id, &rating_key)?;
            existing.insert(rating_key.clone());
            playlist_id = Some(id);
            update.added += 1;
            continue;
        }

        if existing.contains(&rating_key) {
            update.already_present += 1;
            continue;
        }

        if let Some(id) = playlist_id.as_deref() {
            add_item_to_playlist(transport, id, &machine_id, &rating_key)?;
            existing.insert(rating_key);
            update.added += 1;
        }
    }

    update.playlist_id = playlist_id;
    Ok(update)
}

fn audio_tracks(results: &[ItemResult]) -> Vec<TrackCandidate> {
    let mut tracks = Vec::new();
    let mut seen = BTreeSet::new();
    for result in results {
        for file in &result.files {
            if file.kind != "audio" {
                continue;
            }
            let title = file
                .title
                .as_deref()
                .or(result.title.as_deref())
                .or_else(|| file.path.file_stem().and_then(|s| s.to_str()))
                .unwrap_or("")
                .trim();
            if title.is_empty() {
                continue;
            }
            let uploader = file.uploader.clone().or_else(|| result.uploader.clone());
            let key = format!(
                "{}\u{1f}{}",
                title.to_ascii_lowercase(),
                uploader.as_deref().unwrap_or("").to_ascii_lowercase()
            );
            if seen.insert(key) {
                tracks.push(TrackCandidate {
                    title: title.to_string(),
                    uploader,
                });
            }
        }
    }
    tracks
}

fn machine_identifier(transport: &mut impl PlexTransport) -> Result<String> {
    let value = transport.get("/identity", &[])?;
    value
        .pointer("/MediaContainer/machineIdentifier")
        .and_then(Value::as_str)
        .map(str::to_string)
        .context("Plex identity response did not include machineIdentifier")
}

fn find_playlist_id(transport: &mut impl PlexTransport, playlist: &str) -> Result<Option<String>> {
    let value = transport.get("/playlists", &[("playlistType", "audio")])?;
    for item in metadata_items(&value) {
        let rating_key = item
            .get("ratingKey")
            .and_then(Value::as_str)
            .map(str::to_string);
        if rating_key.as_deref() == Some(playlist) {
            return Ok(rating_key);
        }
        let title = item.get("title").and_then(Value::as_str);
        if title.is_some_and(|title| title.eq_ignore_ascii_case(playlist)) {
            return Ok(rating_key);
        }
    }
    Ok(None)
}

fn playlist_item_keys(
    transport: &mut impl PlexTransport,
    playlist_id: &str,
) -> Result<BTreeSet<String>> {
    let value = transport.get(&format!("/playlists/{playlist_id}/items"), &[])?;
    Ok(metadata_items(&value)
        .filter_map(|item| item.get("ratingKey").and_then(Value::as_str))
        .map(str::to_string)
        .collect())
}

fn find_track_rating_key(
    transport: &mut impl PlexTransport,
    track: &TrackCandidate,
) -> Result<Option<String>> {
    let value = transport.get(
        "/search",
        &[("query", track.title.as_str()), ("type", TRACK_TYPE)],
    )?;
    let mut fallback = None;
    for item in metadata_items(&value) {
        let Some(rating_key) = item.get("ratingKey").and_then(Value::as_str) else {
            continue;
        };
        fallback.get_or_insert_with(|| rating_key.to_string());
        let title_matches = item
            .get("title")
            .and_then(Value::as_str)
            .is_some_and(|title| title.eq_ignore_ascii_case(&track.title));
        let uploader_matches = match track.uploader.as_deref() {
            Some(uploader) => ["grandparentTitle", "parentTitle", "originalTitle"]
                .iter()
                .filter_map(|field| item.get(*field).and_then(Value::as_str))
                .any(|artist| artist.eq_ignore_ascii_case(uploader)),
            None => true,
        };
        if title_matches && uploader_matches {
            return Ok(Some(rating_key.to_string()));
        }
    }
    Ok(fallback)
}

fn create_playlist(
    transport: &mut impl PlexTransport,
    playlist: &str,
    machine_id: &str,
    rating_key: &str,
) -> Result<String> {
    let uri = library_uri(machine_id, rating_key);
    let value = transport.post(
        "/playlists",
        &[
            ("type", "audio"),
            ("title", playlist),
            ("smart", "0"),
            ("uri", uri.as_str()),
        ],
    )?;
    let rating_key = metadata_items(&value)
        .find_map(|item| item.get("ratingKey").and_then(Value::as_str))
        .map(str::to_string)
        .context("Plex create playlist response did not include ratingKey")?;
    Ok(rating_key)
}

fn add_item_to_playlist(
    transport: &mut impl PlexTransport,
    playlist_id: &str,
    machine_id: &str,
    rating_key: &str,
) -> Result<()> {
    let uri = library_uri(machine_id, rating_key);
    transport.put(
        &format!("/playlists/{playlist_id}/items"),
        &[("uri", uri.as_str())],
    )
}

fn library_uri(machine_id: &str, rating_key: &str) -> String {
    format!("server://{machine_id}/com.plexapp.plugins.library/library/metadata/{rating_key}")
}

fn metadata_items(value: &Value) -> impl Iterator<Item = &Value> {
    value
        .pointer("/MediaContainer/Metadata")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

trait PlexTransport {
    fn get(&mut self, path: &str, params: &[(&str, &str)]) -> Result<Value>;
    fn post(&mut self, path: &str, params: &[(&str, &str)]) -> Result<Value>;
    fn put(&mut self, path: &str, params: &[(&str, &str)]) -> Result<()>;
}

struct UreqPlexTransport {
    base_url: String,
    token: String,
}

impl UreqPlexTransport {
    fn url(&self, path: &str, params: &[(&str, &str)]) -> Result<Url> {
        let base = format!("{}/", self.base_url.trim_end_matches('/'));
        let mut url = Url::parse(&base).context("parse YTDLP_PLEX_URL")?;
        url.set_path(path.trim_start_matches('/'));
        {
            let mut pairs = url.query_pairs_mut();
            pairs.append_pair("X-Plex-Token", &self.token);
            for (key, value) in params {
                pairs.append_pair(key, value);
            }
        }
        Ok(url)
    }

    fn read_json(&self, mut response: ureq::http::Response<ureq::Body>) -> Result<Value> {
        if !response.status().is_success() {
            bail!("Plex returned HTTP {}", response.status());
        }
        let mut reader = response.body_mut().as_reader();
        serde_json::from_reader(&mut reader).context("parse Plex JSON response")
    }
}

impl PlexTransport for UreqPlexTransport {
    fn get(&mut self, path: &str, params: &[(&str, &str)]) -> Result<Value> {
        let url = self.url(path, params)?;
        let response = ureq::get(url.as_str())
            .header("Accept", "application/json")
            .call()
            .with_context(|| format!("GET {url}"))?;
        self.read_json(response)
    }

    fn post(&mut self, path: &str, params: &[(&str, &str)]) -> Result<Value> {
        let url = self.url(path, params)?;
        let response = ureq::post(url.as_str())
            .header("Accept", "application/json")
            .send_empty()
            .with_context(|| format!("POST {url}"))?;
        self.read_json(response)
    }

    fn put(&mut self, path: &str, params: &[(&str, &str)]) -> Result<()> {
        let url = self.url(path, params)?;
        let response = ureq::put(url.as_str())
            .header("Accept", "application/json")
            .send_empty()
            .with_context(|| format!("PUT {url}"))?;
        if !response.status().is_success() {
            bail!("Plex returned HTTP {}", response.status());
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "plex_tests.rs"]
mod tests;
