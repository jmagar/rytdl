use std::collections::HashMap;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use serde::Serialize;
use serde_json::Value;
use url::Url;

use crate::identify::http_agent;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RetagPreview {
    pub confidence: f64,
    pub recording_id: String,
    pub recording_title: String,
    pub artist: String,
    pub artists: Vec<String>,
    pub release_id: Option<String>,
    pub release_title: Option<String>,
    pub release_group_id: Option<String>,
    pub release_group_title: Option<String>,
    pub release_type: Option<String>,
    pub release_date: Option<String>,
    pub track_number: Option<String>,
    pub musicbrainz_url: String,
}

pub(crate) trait MusicBrainzLookup: Send {
    fn lookup_recording(&mut self, recording_id: &str, score: f64) -> Result<RetagPreview>;
}

pub(crate) struct UreqMusicBrainzLookup {
    user_agent: String,
    agent: ureq::Agent,
    last_call: Option<Instant>,
    /// PerfM4: memoize recordings within a single identify batch so repeated
    /// recording_ids don't re-hit MusicBrainz (and don't pay the 1s rate-limit
    /// sleep again). Keyed by `(recording_id, score-bits)` because the returned
    /// preview embeds `confidence = score`.
    cache: HashMap<(String, u64), RetagPreview>,
}

impl UreqMusicBrainzLookup {
    pub fn new(user_agent: String) -> Self {
        Self {
            user_agent,
            agent: http_agent(),
            last_call: None,
            cache: HashMap::new(),
        }
    }
}

impl MusicBrainzLookup for UreqMusicBrainzLookup {
    fn lookup_recording(&mut self, recording_id: &str, score: f64) -> Result<RetagPreview> {
        let cache_key = (recording_id.to_string(), score.to_bits());
        if let Some(preview) = self.cache.get(&cache_key) {
            return Ok(preview.clone());
        }
        wait_for_rate_limit(&mut self.last_call);
        let url = recording_url(recording_id)?;
        let mut response = self
            .agent
            .get(url.as_str())
            .header("Accept", "application/json")
            .header("User-Agent", &self.user_agent)
            .call()
            .context("call MusicBrainz recording lookup")?;
        if !response.status().is_success() {
            bail!("MusicBrainz returned HTTP {}", response.status());
        }
        let bytes = response
            .body_mut()
            .read_to_vec()
            .context("read MusicBrainz response")?;
        let preview = parse_recording_lookup(&bytes, score)?;
        self.cache.insert(cache_key, preview.clone());
        Ok(preview)
    }
}

fn wait_for_rate_limit(last_call: &mut Option<Instant>) {
    if let Some(last) = *last_call {
        let elapsed = last.elapsed();
        if elapsed < Duration::from_secs(1) {
            std::thread::sleep(Duration::from_secs(1) - elapsed);
        }
    }
    *last_call = Some(Instant::now());
}

fn recording_url(recording_id: &str) -> Result<Url> {
    let mut url = Url::parse(&format!(
        "https://musicbrainz.org/ws/2/recording/{recording_id}"
    ))?;
    url.query_pairs_mut()
        .append_pair("fmt", "json")
        .append_pair("inc", "artist-credits+releases+release-groups+media");
    Ok(url)
}

pub(crate) fn parse_recording_lookup(bytes: &[u8], score: f64) -> Result<RetagPreview> {
    let value: Value = serde_json::from_slice(bytes)?;
    let recording_id = str_field(&value, "id").context("MusicBrainz response missing id")?;
    let recording_title =
        str_field(&value, "title").context("MusicBrainz response missing title")?;
    let artists = artist_credit(&value);
    let artist = if artists.is_empty() {
        "Unknown Artist".to_string()
    } else {
        artists.join(", ")
    };
    let release = first_release(&value);
    let release_group = release.and_then(|r| r.get("release-group"));

    Ok(RetagPreview {
        confidence: score,
        musicbrainz_url: format!("https://musicbrainz.org/recording/{recording_id}"),
        track_number: release.and_then(|r| track_number(r, &recording_id)),
        release_id: release.and_then(|r| str_field(r, "id")),
        release_title: release.and_then(|r| str_field(r, "title")),
        release_date: release.and_then(|r| str_field(r, "date")),
        release_group_id: release_group.and_then(|g| str_field(g, "id")),
        release_group_title: release_group.and_then(|g| str_field(g, "title")),
        release_type: release_group.and_then(release_type),
        recording_id,
        recording_title,
        artist,
        artists,
    })
}

fn artist_credit(value: &Value) -> Vec<String> {
    value["artist-credit"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|credit| {
            str_field(credit, "name")
                .or_else(|| credit.get("artist").and_then(|a| str_field(a, "name")))
        })
        .collect()
}

fn first_release(value: &Value) -> Option<&Value> {
    value["releases"].as_array()?.first()
}

fn track_number(release: &Value, recording_id: &str) -> Option<String> {
    for medium in release["media"].as_array()? {
        for track in medium["tracks"].as_array()? {
            if track["recording"]["id"].as_str().is_none()
                || track["recording"]["id"].as_str() == Some(recording_id)
            {
                return str_field(track, "number");
            }
        }
    }
    None
}

fn release_type(release_group: &Value) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(primary) = str_field(release_group, "primary-type") {
        parts.push(primary);
    }
    for secondary in release_group["secondary-types"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter_map(non_empty)
    {
        if !parts.contains(&secondary) {
            parts.push(secondary);
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}

fn str_field(value: &Value, key: &str) -> Option<String> {
    value[key].as_str().and_then(non_empty)
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
#[path = "musicbrainz_tests.rs"]
mod tests;
