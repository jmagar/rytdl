# Plex Playlist Builder And Transfer Drain Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Plex playlist builder from successful audio download history, generate best-effort Plexamp playback links, and add a safe transfer drain queue for retained staging directories.

**Architecture:** Add two action-dispatched MCP tools: `youtube_plex_playlist` and `youtube_transfer_queue`. Keep history candidate extraction, Plex preview/apply planning, and transfer queue persistence in focused modules so `service.rs` stays orchestration-only. The embedded MCP app gets Playlist and Transfers tabs that call those tools and open/copy Plexamp links through host bridge wrappers.

**Tech Stack:** Rust 2021, `rmcp`, `schemars`, `serde`, `serde_json`, `sha2`, `ureq`, Tokio, plain embedded HTML/CSS/JS, MCP Apps metadata plus OpenAI Apps compatibility metadata.

## Global Constraints

- Playlist candidates MUST come only from history entries where `transferred == true`.
- Playlist candidates MUST include audio files only.
- Candidate IDs MUST be stable opaque strings and MUST NOT be array indexes or history line numbers.
- Plex preview and apply MUST share one resolver path; preview MUST NOT call Plex mutation endpoints.
- Plex apply MUST be idempotent; existing items count as `already_present`.
- Plexamp links MUST be marked best-effort/generated-unverified because the `listen.plex.tv` URL shape is community-proven, not an official Plexamp API guarantee.
- User-facing Plexamp/Plex Web links MUST be built only from `machineIdentifier`, playlist IDs, and playlist keys. They MUST NOT reuse Plex request URLs or include `X-Plex-Token`.
- Transfer drains MUST accept opaque manifest IDs only; they MUST NOT accept arbitrary filesystem paths.
- Transfer queue manifests MUST be written at transfer failure time, while `results`, `transfer_dests`, `staging_path`, and original targets still coexist.
- Transfer queue locks MUST fail closed on lock/open/acquire failure.
- Transfer queue manifest writes MUST use same-directory temp file, file sync, atomic rename, and directory sync.
- Drain retry MUST re-parse target paths and re-check `YTDLP_ALLOW_LOCAL_TARGETS` before transfer.
- Transfer queue `last_error`, MCP responses, logs, and app-visible errors MUST redact sensitive subprocess output before persistence or rendering.
- MCP app external links MUST validate allowed origins before open/copy and MUST allow only `https://listen.plex.tv` and `https://app.plex.tv` for Plex playback links.
- External link metadata MUST include both MCP Apps CSP domains and OpenAI `openai/widgetCSP.redirect_domains`.
- Do not add new runtime dependencies unless a task explicitly says to; use existing `sha2`.
- Keep files under the repo convention: no `mod.rs`, sibling test files where practical, and no file over 500 LOC without splitting.

---

### Task 1: Successful Audio History Candidates

**Files:**
- Modify: `src/history.rs`
- Create: `src/history/candidates.rs`
- Modify: `src/history_tests.rs`
- Modify: `src/model.rs`
- Modify: `src/service.rs`
- Modify: `src/mcp.rs`
- Modify: `src/mcp_tests.rs`

**Interfaces:**
- Produces: `history::playlist_candidates(cfg: &Config, limit: usize) -> anyhow::Result<PlaylistCandidatesPayload>`
- Produces DTOs:
  - `PlaylistCandidatesPayload { history_path: String, skipped_entries: u64, candidates: Vec<PlaylistCandidate> }`
  - `PlaylistCandidate { candidate_id: String, title: String, uploader: Option<String>, video_id: Option<String>, url: String, timestamp: String, duration: Option<f64>, bytes: u64 }`
- Produces input model:
  - `PlexPlaylistInput { action: PlexPlaylistAction, playlist: Option<String>, candidate_ids: Vec<String>, limit: Option<u32>, response_format: ResponseFormat }`
  - `PlexPlaylistAction::{ListCandidates, Preview, Apply}`
- Consumed by Task 2 for preview/apply lookup.

- [ ] **Step 1: Write failing candidate extraction tests**

Add tests to `src/history_tests.rs`:

```rust
#[test]
fn playlist_candidates_include_only_transferred_audio() {
    let dir = tempfile::tempdir().unwrap();
    let history = dir.path().join("downloads.jsonl");
    std::fs::write(
        &history,
        concat!(
            "{\"timestamp\":\"2026-07-12T01:00:00Z\",\"mode\":\"audio\",\"target_path\":\"tootie:/music\",\"transferred\":true,\"total_files\":1,\"total_bytes\":10,\"items\":[{\"url\":\"https://youtu.be/a\",\"status\":\"ok\",\"title\":\"Song A\",\"uploader\":\"Artist A\",\"video_id\":\"aaa\",\"duration\":12.0,\"files\":[{\"kind\":\"audio\",\"bytes\":10,\"title\":\"Song A\",\"uploader\":\"Artist A\",\"video_id\":\"aaa\",\"duration\":12.0}]}]}\n",
            "{\"timestamp\":\"2026-07-12T01:01:00Z\",\"mode\":\"audio\",\"target_path\":\"tootie:/music\",\"transferred\":false,\"total_files\":1,\"total_bytes\":20,\"items\":[{\"url\":\"https://youtu.be/b\",\"status\":\"ok\",\"title\":\"Song B\",\"uploader\":\"Artist B\",\"video_id\":\"bbb\",\"files\":[{\"kind\":\"audio\",\"bytes\":20,\"title\":\"Song B\",\"uploader\":\"Artist B\",\"video_id\":\"bbb\"}]}]}\n",
            "{\"timestamp\":\"2026-07-12T01:02:00Z\",\"mode\":\"video\",\"target_path\":\"tootie:/video\",\"transferred\":true,\"total_files\":1,\"total_bytes\":30,\"items\":[{\"url\":\"https://youtu.be/c\",\"status\":\"ok\",\"title\":\"Video C\",\"uploader\":\"Artist C\",\"video_id\":\"ccc\",\"files\":[{\"kind\":\"video\",\"bytes\":30,\"title\":\"Video C\",\"uploader\":\"Artist C\",\"video_id\":\"ccc\"}]}]}\n",
            "not-json\n"
        ),
    )
    .unwrap();
    let mut cfg = test_config();
    cfg.history_path = Some(history.display().to_string());

    let payload = crate::history::playlist_candidates(&cfg, 25).unwrap();

    assert_eq!(payload.skipped_entries, 1);
    assert_eq!(payload.candidates.len(), 1);
    assert_eq!(payload.candidates[0].title, "Song A");
    assert_eq!(payload.candidates[0].uploader.as_deref(), Some("Artist A"));
    assert_eq!(payload.candidates[0].video_id.as_deref(), Some("aaa"));
    assert_eq!(payload.candidates[0].bytes, 10);
    assert!(!payload.candidates[0].candidate_id.is_empty());
}

#[test]
fn playlist_candidates_dedupe_on_normalized_track_identity() {
    let dir = tempfile::tempdir().unwrap();
    let history = dir.path().join("downloads.jsonl");
    let line = "{\"timestamp\":\"2026-07-12T01:00:00Z\",\"mode\":\"audio\",\"target_path\":\"tootie:/music\",\"transferred\":true,\"total_files\":1,\"total_bytes\":10,\"items\":[{\"url\":\"https://youtu.be/a\",\"status\":\"ok\",\"title\":\"Song A\",\"uploader\":\"Artist A\",\"video_id\":\"aaa\",\"files\":[{\"kind\":\"audio\",\"bytes\":10,\"title\":\"Song A\",\"uploader\":\"Artist A\",\"video_id\":\"aaa\"}]}]}\n";
    std::fs::write(&history, format!("{line}{line}")).unwrap();
    let mut cfg = test_config();
    cfg.history_path = Some(history.display().to_string());

    let payload = crate::history::playlist_candidates(&cfg, 25).unwrap();

    assert_eq!(payload.candidates.len(), 1);
}
```

- [ ] **Step 2: Run tests to verify failure**

Run:

```bash
cargo test playlist_candidates -- --nocapture
```

Expected: FAIL because `crate::history::playlist_candidates` and DTOs do not exist.

- [ ] **Step 3: Add focused candidate module**

In `src/history.rs`, add near the module declarations/imports:

```rust
mod candidates;

pub(crate) use candidates::{playlist_candidates, PlaylistCandidate, PlaylistCandidatesPayload};
```

Create `src/history/candidates.rs`:

```rust
use std::collections::BTreeSet;
use std::fs::File;
use std::io::{BufRead, BufReader, ErrorKind};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::config::Config;
use crate::history::{history_path, HistoryLock};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq)]
pub(crate) struct PlaylistCandidatesPayload {
    pub history_path: String,
    pub skipped_entries: u64,
    pub candidates: Vec<PlaylistCandidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq)]
pub(crate) struct PlaylistCandidate {
    pub candidate_id: String,
    pub title: String,
    pub uploader: Option<String>,
    pub video_id: Option<String>,
    pub url: String,
    pub timestamp: String,
    pub duration: Option<f64>,
    pub bytes: u64,
}

pub(crate) fn playlist_candidates(
    cfg: &Config,
    limit: usize,
) -> Result<PlaylistCandidatesPayload> {
    let path = history_path(cfg);
    let _guard = HistoryLock::acquire(&path);
    let file = match File::open(&path) {
        Ok(file) => Some(file),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => {
            return Err(error).with_context(|| format!("open history file {}", path.display()))
        }
    };

    let mut candidates = Vec::new();
    let mut seen = BTreeSet::new();
    let mut skipped_entries = 0_u64;

    if let Some(file) = file {
        for line in BufReader::new(file).lines() {
            let line = line.with_context(|| format!("read history file {}", path.display()))?;
            if line.trim().is_empty() {
                continue;
            }
            let entry: Value = match serde_json::from_str(&line) {
                Ok(entry) => entry,
                Err(error) => {
                    skipped_entries += 1;
                    tracing::warn!(%error, "skipping malformed download history entry");
                    continue;
                }
            };
            if entry["transferred"].as_bool() != Some(true) {
                continue;
            }
            let timestamp = entry["timestamp"].as_str().unwrap_or("").to_string();
            for item in entry["items"].as_array().into_iter().flatten() {
                collect_item_candidates(item, &timestamp, &mut seen, &mut candidates);
                if limit > 0 && candidates.len() >= limit {
                    return Ok(payload(path, skipped_entries, candidates));
                }
            }
        }
    }

    Ok(payload(path, skipped_entries, candidates))
}

fn collect_item_candidates(
    item: &Value,
    timestamp: &str,
    seen: &mut BTreeSet<String>,
    candidates: &mut Vec<PlaylistCandidate>,
) {
    let url = item["url"].as_str().unwrap_or("").to_string();
    for file in item["files"].as_array().into_iter().flatten() {
        if file["kind"].as_str() != Some("audio") {
            continue;
        }
        let title = file["title"]
            .as_str()
            .or_else(|| item["title"].as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if title.is_empty() {
            continue;
        }
        let uploader = file["uploader"]
            .as_str()
            .or_else(|| item["uploader"].as_str())
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string);
        let video_id = file["video_id"]
            .as_str()
            .or_else(|| item["video_id"].as_str())
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string);
        let key = normalized_key(&title, uploader.as_deref(), video_id.as_deref(), &url);
        if !seen.insert(key.clone()) {
            continue;
        }
        candidates.push(PlaylistCandidate {
            candidate_id: candidate_id(&key),
            title,
            uploader,
            video_id,
            url: url.clone(),
            timestamp: timestamp.to_string(),
            duration: file["duration"].as_f64().or_else(|| item["duration"].as_f64()),
            bytes: file["bytes"].as_u64().unwrap_or(0),
        });
    }
}

fn normalized_key(title: &str, uploader: Option<&str>, video_id: Option<&str>, url: &str) -> String {
    format!(
        "{}\u{1f}{}\u{1f}{}\u{1f}{}",
        title.trim().to_ascii_lowercase(),
        uploader.unwrap_or("").trim().to_ascii_lowercase(),
        video_id.unwrap_or("").trim().to_ascii_lowercase(),
        url.trim().to_ascii_lowercase()
    )
}

fn candidate_id(key: &str) -> String {
    let digest = Sha256::digest(key.as_bytes());
    format!("pc_{digest:x}")
}

fn payload(
    path: std::path::PathBuf,
    skipped_entries: u64,
    candidates: Vec<PlaylistCandidate>,
) -> PlaylistCandidatesPayload {
    PlaylistCandidatesPayload {
        history_path: path.display().to_string(),
        skipped_entries,
        candidates,
    }
}
```

If `history_path` or `HistoryLock` are private, change their visibility in `src/history.rs` from private to `pub(crate)`.

- [ ] **Step 4: Add action input model**

In `src/model.rs`, add:

```rust
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlexPlaylistAction {
    ListCandidates,
    Preview,
    Apply,
}

fn default_playlist_limit() -> u32 {
    100
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct PlexPlaylistInput {
    #[serde(default = "default_plex_playlist_action")]
    pub action: PlexPlaylistAction,
    #[serde(default)]
    pub playlist: Option<String>,
    #[serde(default)]
    pub candidate_ids: Vec<String>,
    #[serde(default = "default_playlist_limit")]
    pub limit: u32,
    #[serde(default)]
    pub response_format: ResponseFormat,
}

fn default_plex_playlist_action() -> PlexPlaylistAction {
    PlexPlaylistAction::ListCandidates
}
```

- [ ] **Step 5: Expose list-candidates tool path**

In `src/service.rs`, add:

```rust
pub fn run_plex_playlist(
    cfg: &Config,
    input: crate::model::PlexPlaylistInput,
) -> Result<String> {
    match input.action {
        crate::model::PlexPlaylistAction::ListCandidates => {
            let payload = crate::history::playlist_candidates(cfg, input.limit as usize)?;
            render(
                &serde_json::to_value(&payload)?,
                input.response_format,
                crate::history::render_playlist_candidates_markdown,
            )
        }
        crate::model::PlexPlaylistAction::Preview | crate::model::PlexPlaylistAction::Apply => {
            bail!("Plex playlist {:?} is not implemented yet", input.action);
        }
    }
}
```

Add `render_playlist_candidates_markdown` in `src/history/candidates.rs`:

```rust
pub(crate) fn render_playlist_candidates_markdown(payload: &serde_json::Value) -> String {
    let count = payload["candidates"].as_array().map_or(0, Vec::len);
    let mut lines = vec![format!("{count} Plex playlist candidate(s).")];
    for item in payload["candidates"].as_array().into_iter().flatten().take(10) {
        let title = item["title"].as_str().unwrap_or("Untitled");
        let uploader = item["uploader"].as_str().unwrap_or("Unknown artist");
        lines.push(format!("- {title} - {uploader}"));
    }
    lines.join("\n")
}
```

Re-export it from `src/history.rs`.

In `src/mcp.rs`, add a `youtube_plex_playlist` tool using the existing `structured_tool_result` pattern and mark it app-callable with `search_app::app_callable_tool_meta()`.

- [ ] **Step 6: Run candidate tests**

Run:

```bash
cargo test playlist_candidates -- --nocapture
cargo test mcp -- --nocapture
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/history.rs src/history/candidates.rs src/history_tests.rs src/model.rs src/service.rs src/mcp.rs src/mcp_tests.rs
git commit -m "feat: list Plex playlist candidates from history"
```

### Task 2: Plex Preview, Apply, And Deep Links

**Files:**
- Modify: `src/plex.rs`
- Create: `src/plex/playlist.rs`
- Modify: `src/plex_tests.rs`
- Modify: `src/service.rs`
- Modify: `src/service_tests.rs`
- Modify: `src/mcp_tests.rs`

**Interfaces:**
- Consumes: `history::playlist_candidates`
- Produces: `plex::preview_playlist(...) -> Result<PlexPlaylistPlan>`
- Produces: `plex::apply_playlist(...) -> Result<PlexPlaylistPlan>`
- Produces response fields:
  - `playlist`, `playlist_id`, `machine_identifier`, `matched`, `added`, `already_present`, `missing`, `errors`
  - `plexamp_url`, `plex_web_url`, `playback_link_status: "generated_unverified"`

- [ ] **Step 1: Write failing Plex preview/apply/link tests**

Add tests to `src/plex_tests.rs` using the existing fake transport style:

```rust
#[test]
fn preview_playlist_does_not_mutate_plex() {
    let mut plex = FakePlex::new()
        .with_identity("machine-1")
        .with_playlist("99", "Road Trip", &[])
        .with_search_result("Song A", "11", "Song A", "Artist A");
    let tracks = vec![PlexTrackInput {
        title: "Song A".into(),
        uploader: Some("Artist A".into()),
    }];

    let result = preview_audio_tracks_with_transport(&mut plex, "Road Trip", &tracks).unwrap();

    assert_eq!(result.matched, 1);
    assert_eq!(result.added, 0);
    assert_eq!(result.already_present, 0);
    assert!(plex.posts.is_empty());
    assert!(plex.puts.is_empty());
}

#[test]
fn apply_playlist_returns_best_effort_plexamp_link() {
    let mut plex = FakePlex::new()
        .with_identity("machine-1")
        .with_playlist("99", "Road Trip", &[])
        .with_search_result("Song A", "11", "Song A", "Artist A");
    let tracks = vec![PlexTrackInput {
        title: "Song A".into(),
        uploader: Some("Artist A".into()),
    }];

    let result = apply_audio_tracks_with_transport(&mut plex, "Road Trip", &tracks).unwrap();

    assert_eq!(result.playlist_id.as_deref(), Some("99"));
    assert_eq!(result.playback_link_status.as_deref(), Some("generated_unverified"));
    assert!(result.plexamp_url.as_deref().unwrap().starts_with(
        "https://listen.plex.tv/player/playback/playMedia?uri="
    ));
    assert!(!result.plexamp_url.unwrap().contains("X-Plex-Token"));
}
```

- [ ] **Step 2: Run tests to verify failure**

```bash
cargo test plex -- --nocapture
```

Expected: FAIL because preview/apply helpers and link fields do not exist.

- [ ] **Step 3: Split Plex planner**

In `src/plex.rs`, add:

```rust
mod playlist;

pub use playlist::{
    apply_audio_tracks, preview_audio_tracks, PlexPlaylistActionResult, PlexPlaybackLinks,
};

#[cfg(test)]
pub(crate) use playlist::{
    apply_audio_tracks_with_transport, preview_audio_tracks_with_transport,
};
```

Create `src/plex/playlist.rs` with:

```rust
use anyhow::{Context, Result};
use serde::Serialize;
use urlencoding::encode;

use super::{
    dedup_tracks, find_track_rating_key, machine_identifier, PlaylistState, PlexMissingTrack,
    PlexTrackInput, PlexTransport, TrackCandidate,
};
use crate::config::Config;

#[derive(Debug, Clone, Serialize, schemars::JsonSchema, PartialEq)]
pub struct PlexPlaylistActionResult {
    pub playlist: String,
    pub playlist_id: Option<String>,
    pub machine_identifier: Option<String>,
    pub matched: usize,
    pub added: usize,
    pub already_present: usize,
    pub missing: Vec<PlexMissingTrack>,
    pub errors: Vec<String>,
    pub plexamp_url: Option<String>,
    pub plex_web_url: Option<String>,
    pub playback_link_status: Option<String>,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema, PartialEq)]
pub struct PlexPlaybackLinks {
    pub plexamp_url: String,
    pub plex_web_url: String,
    pub playback_link_status: String,
}

pub fn preview_audio_tracks(
    cfg: &Config,
    playlist: &str,
    tracks: &[PlexTrackInput],
) -> Result<PlexPlaylistActionResult> {
    let mut transport = super::transport_from_config(cfg)?;
    preview_audio_tracks_with_transport(&mut transport, playlist, tracks)
}

pub fn apply_audio_tracks(
    cfg: &Config,
    playlist: &str,
    tracks: &[PlexTrackInput],
) -> Result<PlexPlaylistActionResult> {
    let mut transport = super::transport_from_config(cfg)?;
    apply_audio_tracks_with_transport(&mut transport, playlist, tracks)
}

pub(crate) fn preview_audio_tracks_with_transport(
    transport: &mut impl PlexTransport,
    playlist: &str,
    tracks: &[PlexTrackInput],
) -> Result<PlexPlaylistActionResult> {
    run_playlist_plan(transport, playlist, dedup_tracks(tracks), false)
}

pub(crate) fn apply_audio_tracks_with_transport(
    transport: &mut impl PlexTransport,
    playlist: &str,
    tracks: &[PlexTrackInput],
) -> Result<PlexPlaylistActionResult> {
    run_playlist_plan(transport, playlist, dedup_tracks(tracks), true)
}

fn run_playlist_plan(
    transport: &mut impl PlexTransport,
    playlist: &str,
    tracks: Vec<TrackCandidate>,
    mutate: bool,
) -> Result<PlexPlaylistActionResult> {
    let machine_id = machine_identifier(transport)?;
    let mut state = PlaylistState::resolve(transport, playlist)?;
    let mut result = PlexPlaylistActionResult {
        playlist: playlist.to_string(),
        playlist_id: state.id().map(str::to_string),
        machine_identifier: Some(machine_id.clone()),
        matched: 0,
        added: 0,
        already_present: 0,
        missing: Vec::new(),
        errors: Vec::new(),
        plexamp_url: None,
        plex_web_url: None,
        playback_link_status: None,
    };

    for track in tracks {
        let Some(rating_key) = find_track_rating_key(transport, &track)? else {
            result.missing.push(PlexMissingTrack {
                title: track.title,
                uploader: track.uploader,
            });
            continue;
        };
        result.matched += 1;
        if state.contains(&rating_key) {
            result.already_present += 1;
            continue;
        }
        if mutate {
            state.add_track(transport, playlist, &machine_id, &rating_key)?;
            result.added += 1;
        }
    }

    result.playlist_id = state.into_id();
    if let Some(playlist_id) = result.playlist_id.as_deref() {
        let links = playback_links(&machine_id, playlist_id);
        result.plexamp_url = Some(links.plexamp_url);
        result.plex_web_url = Some(links.plex_web_url);
        result.playback_link_status = Some(links.playback_link_status);
    }
    Ok(result)
}

pub fn playback_links(machine_id: &str, playlist_id: &str) -> PlexPlaybackLinks {
    let server_uri = format!(
        "server://{machine_id}/com.plexapp.plugins.library/playlists/{playlist_id}/items"
    );
    PlexPlaybackLinks {
        plexamp_url: format!(
            "https://listen.plex.tv/player/playback/playMedia?uri={}",
            encode(&server_uri)
        ),
        plex_web_url: format!(
            "https://app.plex.tv/desktop/#!/server/{}/playlist?key={}",
            encode(machine_id),
            encode(&format!("/playlists/{playlist_id}/items"))
        ),
        playback_link_status: "generated_unverified".to_string(),
    }
}
```

Adjust visibilities in `src/plex.rs` to `pub(crate)` for planner-only helpers: `PlexTransport`, `TrackCandidate`, `dedup_tracks`, `machine_identifier`, `find_track_rating_key`, `PlaylistState`, and add `PlaylistState::id(&self) -> Option<&str>`.

Extract existing config transport creation into:

```rust
pub(crate) fn transport_from_config(cfg: &Config) -> Result<UreqPlexTransport> {
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
    Ok(UreqPlexTransport {
        base_url: base_url.to_string(),
        token: token.to_string(),
    })
}
```

Keep `add_downloaded_audio` by delegating to `apply_audio_tracks` and mapping back to the existing `PlexPlaylistUpdate` if needed for backward-compatible download payloads.

- [ ] **Step 4: Wire service preview/apply from history candidates**

In `src/service.rs`, replace the temporary unsupported branches in `run_plex_playlist`:

```rust
crate::model::PlexPlaylistAction::Preview | crate::model::PlexPlaylistAction::Apply => {
    let playlist = input
        .playlist
        .clone()
        .or_else(|| cfg.plex_playlist.clone())
        .unwrap_or_else(|| crate::config::DEFAULT_PLEX_PLAYLIST.to_string());
    let candidates = crate::history::playlist_candidates(cfg, input.limit as usize)?;
    let wanted: std::collections::BTreeSet<&str> =
        input.candidate_ids.iter().map(String::as_str).collect();
    let tracks: Vec<crate::plex::PlexTrackInput> = candidates
        .candidates
        .iter()
        .filter(|candidate| wanted.is_empty() || wanted.contains(candidate.candidate_id.as_str()))
        .map(|candidate| crate::plex::PlexTrackInput {
            title: candidate.title.clone(),
            uploader: candidate.uploader.clone(),
        })
        .collect();
    let result = match input.action {
        crate::model::PlexPlaylistAction::Preview => {
            crate::plex::preview_audio_tracks(cfg, &playlist, &tracks)?
        }
        crate::model::PlexPlaylistAction::Apply => {
            crate::plex::apply_audio_tracks(cfg, &playlist, &tracks)?
        }
        crate::model::PlexPlaylistAction::ListCandidates => unreachable!(),
    };
    render(
        &serde_json::to_value(&result)?,
        input.response_format,
        render_plex_playlist_markdown,
    )
}
```

Add `render_plex_playlist_markdown`:

```rust
fn render_plex_playlist_markdown(value: &serde_json::Value) -> String {
    let playlist = value["playlist"].as_str().unwrap_or("Plex playlist");
    let matched = value["matched"].as_u64().unwrap_or(0);
    let added = value["added"].as_u64().unwrap_or(0);
    let already = value["already_present"].as_u64().unwrap_or(0);
    let mut lines = vec![format!(
        "{playlist}: {matched} matched, {added} added, {already} already present."
    )];
    if let Some(url) = value["plexamp_url"].as_str() {
        lines.push(format!("Plexamp: {url}"));
    }
    lines.join("\n")
}
```

- [ ] **Step 5: Run tests**

```bash
cargo test plex -- --nocapture
cargo test playlist_candidates -- --nocapture
cargo test mcp -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/plex.rs src/plex/playlist.rs src/plex_tests.rs src/service.rs src/service_tests.rs src/mcp_tests.rs
git commit -m "feat: preview and apply Plex playlists"
```

### Task 3: Durable Transfer Queue And Drain

**Files:**
- Create: `src/transfer_queue.rs`
- Create: `src/transfer_queue_tests.rs`
- Modify: `src/main.rs`
- Modify: `src/service.rs`
- Modify: `src/transfer.rs`
- Modify: `src/model.rs`
- Modify: `src/mcp.rs`
- Modify: `src/service_tests.rs`
- Modify: `src/mcp_tests.rs`

**Interfaces:**
- Produces: `transfer_queue::record_failed_transfer(cfg, manifest_input) -> Result<TransferQueueEntry>`
- Produces: `service::run_transfer_queue(cfg: &Config, input: TransferQueueInput) -> Result<String>`
- Produces input model:
  - `TransferQueueInput { action: TransferQueueAction, manifest_id: Option<String>, keep_local: bool, response_format: ResponseFormat }`
  - `TransferQueueAction::{List, Retry, RetryAll, Prune}`

- [ ] **Step 1: Write failing queue tests**

Create `src/transfer_queue_tests.rs`:

```rust
use std::path::PathBuf;

use crate::transfer_queue::{
    list_queue, prune_missing, record_failed_transfer, TransferFailureManifestInput,
};

#[test]
fn record_failed_transfer_writes_manifest_with_opaque_id() {
    let dir = tempfile::tempdir().unwrap();
    let staging = dir.path().join("stage");
    std::fs::create_dir_all(staging.join("audio")).unwrap();
    let mut cfg = crate::config_tests::test_config();
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
    let mut cfg = crate::config_tests::test_config();
    cfg.history_path = Some(dir.path().join("downloads.jsonl").display().to_string());

    let kept = record_failed_transfer(&cfg, TransferFailureManifestInput {
        staging_path: staging.clone(),
        targets: vec![("audio".into(), "tootie:/music".into())],
        files: vec![PathBuf::from("audio/A/B.mp3")],
        last_error: "failed".into(),
    }).unwrap();
    std::fs::remove_dir_all(&staging).unwrap();

    let result = prune_missing(&cfg).unwrap();

    assert_eq!(result.pruned, 1);
    assert!(!kept.manifest_path.exists());
    assert!(list_queue(&cfg).unwrap().entries.is_empty());
}
```

- [ ] **Step 2: Run tests to verify failure**

```bash
cargo test transfer_queue -- --nocapture
```

Expected: FAIL because `transfer_queue` module does not exist.

- [ ] **Step 3: Add transfer queue module skeleton**

In `src/main.rs`, add:

```rust
mod transfer_queue;
```

Create `src/transfer_queue.rs`:

```rust
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::config::Config;

const MANIFEST_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub(crate) struct TransferFailureManifestInput {
    pub staging_path: PathBuf,
    pub targets: Vec<(String, String)>,
    pub files: Vec<PathBuf>,
    pub last_error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq)]
pub(crate) struct TransferQueueEntry {
    pub version: u32,
    pub manifest_id: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    pub staging_path: String,
    pub targets: Vec<TransferQueueTarget>,
    pub files: Vec<String>,
    pub attempts: u32,
    pub last_error: Option<String>,
    #[serde(skip)]
    pub manifest_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq)]
pub(crate) struct TransferQueueTarget {
    pub kind: String,
    pub target_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq)]
pub(crate) struct TransferQueueList {
    pub queue_dir: String,
    pub entries: Vec<TransferQueueEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq)]
pub(crate) struct TransferQueuePruneResult {
    pub pruned: usize,
}

pub(crate) fn record_failed_transfer(
    cfg: &Config,
    input: TransferFailureManifestInput,
) -> Result<TransferQueueEntry> {
    let queue_dir = queue_dir(cfg)?;
    fs::create_dir_all(&queue_dir)?;
    let _lock = QueueLock::acquire(&queue_dir)?;
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let manifest_id = manifest_id(&input.staging_path, &now);
    let manifest_path = queue_dir.join(format!("{manifest_id}.json"));
    let entry = TransferQueueEntry {
        version: MANIFEST_VERSION,
        manifest_id,
        status: "pending".to_string(),
        created_at: now.clone(),
        updated_at: now,
        staging_path: input.staging_path.display().to_string(),
        targets: input
            .targets
            .into_iter()
            .map(|(kind, target_path)| TransferQueueTarget { kind, target_path })
            .collect(),
        files: input.files.into_iter().map(|p| p.display().to_string()).collect(),
        attempts: 0,
        last_error: Some(redact_transfer_error(&input.last_error)),
        manifest_path,
    };
    write_manifest(&entry)?;
    Ok(entry)
}

pub(crate) fn list_queue(cfg: &Config) -> Result<TransferQueueList> {
    let queue_dir = queue_dir(cfg)?;
    let _lock = QueueLock::acquire(&queue_dir)?;
    let mut entries = Vec::new();
    if queue_dir.is_dir() {
        for dir_entry in fs::read_dir(&queue_dir)? {
            let path = dir_entry?.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                entries.push(read_manifest(&path)?);
            }
        }
    }
    entries.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    Ok(TransferQueueList {
        queue_dir: queue_dir.display().to_string(),
        entries,
    })
}

pub(crate) fn prune_missing(cfg: &Config) -> Result<TransferQueuePruneResult> {
    let queue_dir = queue_dir(cfg)?;
    let _lock = QueueLock::acquire(&queue_dir)?;
    let mut pruned = 0;
    if queue_dir.is_dir() {
        for dir_entry in fs::read_dir(&queue_dir)? {
            let path = dir_entry?.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let entry = read_manifest(&path)?;
            if !Path::new(&entry.staging_path).is_dir() {
                fs::remove_file(&path)?;
                pruned += 1;
            }
        }
    }
    Ok(TransferQueuePruneResult { pruned })
}

pub(crate) fn queue_dir(cfg: &Config) -> Result<PathBuf> {
    let base = cfg
        .history_path
        .as_ref()
        .map(PathBuf::from)
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| dirs::state_dir().unwrap_or_else(std::env::temp_dir).join("ytdl-rmcp"));
    Ok(base.join("transfer-queue"))
}

pub(crate) fn redact_transfer_error(error: &str) -> String {
    let mut value = error.to_string();
    for marker in ["token=", "password=", "secret=", "key="] {
        while let Some(start) = value.to_ascii_lowercase().find(marker) {
            let end = value[start..]
                .find(char::is_whitespace)
                .map(|offset| start + offset)
                .unwrap_or(value.len());
            value.replace_range(start..end, &format!("{marker}REDACTED"));
        }
    }
    value
}

fn write_manifest(entry: &TransferQueueEntry) -> Result<()> {
    let parent = entry.manifest_path.parent().context("manifest path has no parent")?;
    fs::create_dir_all(parent)?;
    let temp = entry.manifest_path.with_extension("json.tmp");
    {
        let mut file = File::create(&temp)?;
        serde_json::to_writer_pretty(&mut file, entry)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
    }
    fs::rename(&temp, &entry.manifest_path)?;
    let dir = File::open(parent)?;
    dir.sync_all()?;
    Ok(())
}

fn read_manifest(path: &Path) -> Result<TransferQueueEntry> {
    let file = File::open(path)?;
    let mut entry: TransferQueueEntry = serde_json::from_reader(file)?;
    entry.manifest_path = path.to_path_buf();
    if entry.version != MANIFEST_VERSION {
        bail!("unsupported transfer queue manifest version {}", entry.version);
    }
    Ok(entry)
}

fn manifest_id(staging_path: &Path, created_at: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(staging_path.display().to_string().as_bytes());
    hasher.update(b"\0");
    hasher.update(created_at.as_bytes());
    format!("tq_{:x}", hasher.finalize())
}

struct QueueLock(File);

impl QueueLock {
    fn acquire(queue_dir: &Path) -> Result<Self> {
        fs::create_dir_all(queue_dir)?;
        let lock_path = queue_dir.join(".lock");
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .with_context(|| format!("open transfer queue lock {}", lock_path.display()))?;
        fs4::FileExt::lock_exclusive(&file)
            .with_context(|| format!("lock transfer queue {}", lock_path.display()))?;
        Ok(Self(file))
    }
}
```

If `fs4` is not already a dependency, reuse the same locking crate used by `history.rs`.

- [ ] **Step 4: Integrate failure manifest writing**

In `src/service.rs`, after `staging_kept` is created and before `build_download_payload`, call:

```rust
if !transferred {
    if let Some(kept) = staging_kept.as_deref() {
        let queue_input = crate::transfer_queue::TransferFailureManifestInput {
            staging_path: kept.to_path_buf(),
            targets: transfer_dests
                .iter()
                .map(|(kind, target)| ((*kind).to_string(), target.display()))
                .collect(),
            files: results
                .iter()
                .flat_map(|item| item.files.iter())
                .filter_map(|file| file.path.strip_prefix(&staging_path).ok().map(PathBuf::from))
                .collect(),
            last_error: transfer_error.clone().unwrap_or_else(|| "transfer failed".to_string()),
        };
        if let Err(error) = crate::transfer_queue::record_failed_transfer(cfg, queue_input) {
            tracing::warn!(%error, "failed to record transfer queue manifest");
        }
    }
}
```

Add `use std::path::PathBuf;` if needed.

- [ ] **Step 5: Add queue action models and service runner**

In `src/model.rs`, add:

```rust
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TransferQueueAction {
    List,
    Retry,
    RetryAll,
    Prune,
}

fn default_transfer_queue_action() -> TransferQueueAction {
    TransferQueueAction::List
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct TransferQueueInput {
    #[serde(default = "default_transfer_queue_action")]
    pub action: TransferQueueAction,
    #[serde(default)]
    pub manifest_id: Option<String>,
    #[serde(default)]
    pub keep_local: bool,
    #[serde(default)]
    pub response_format: ResponseFormat,
}
```

In `src/service.rs`, add:

```rust
pub fn run_transfer_queue(cfg: &Config, input: crate::model::TransferQueueInput) -> Result<String> {
    let value = match input.action {
        crate::model::TransferQueueAction::List => {
            serde_json::to_value(crate::transfer_queue::list_queue(cfg)?)?
        }
        crate::model::TransferQueueAction::Prune => {
            serde_json::to_value(crate::transfer_queue::prune_missing(cfg)?)?
        }
        crate::model::TransferQueueAction::Retry | crate::model::TransferQueueAction::RetryAll => {
            bail!("transfer queue retry is not implemented until this task wires transfer retry");
        }
    };
    Ok(render(
        &value,
        input.response_format,
        render_transfer_queue_markdown,
    ))
}

fn render_transfer_queue_markdown(value: &serde_json::Value) -> String {
    if let Some(entries) = value["entries"].as_array() {
        return format!("{} pending transfer queue item(s).", entries.len());
    }
    if let Some(pruned) = value["pruned"].as_u64() {
        return format!("Pruned {pruned} transfer queue item(s).");
    }
    "Transfer queue action complete.".to_string()
}
```

Then implement retry/retry_all in `transfer_queue.rs` by loading manifests by opaque ID from the queue directory, canonicalizing `staging_path`, rejecting missing staging dirs with a clear error, re-parsing each target with `TargetPath::parse` or `TransferTarget::parse_targets`, re-checking local target policy, calling `transfer_to_target` for each kind directory, incrementing `attempts`, redacting `last_error`, and deleting the manifest plus staging dir on success unless `keep_local` is true.

- [ ] **Step 6: Expose `youtube_transfer_queue` tool**

In `src/mcp.rs`, add a `youtube_transfer_queue` tool using existing app-callable metadata and `structured_tool_result`.

- [ ] **Step 7: Run queue tests**

```bash
cargo test transfer_queue -- --nocapture
cargo test run_download_json_retains_staging_when_transfer_fails -- --nocapture
cargo test mcp -- --nocapture
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add src/main.rs src/transfer_queue.rs src/transfer_queue_tests.rs src/service.rs src/transfer.rs src/model.rs src/mcp.rs src/service_tests.rs src/mcp_tests.rs
git commit -m "feat: add transfer drain queue"
```

### Task 4: MCP App Playlist And Transfers Tabs

**Files:**
- Modify: `assets/youtube-search-app.html`
- Create/Modify: `assets/youtube-search-app.js`
- Modify: `src/search_app.rs`
- Modify: `src/search_app_tests.rs`
- Modify: `src/mcp.rs`
- Modify: `src/mcp_tests.rs`

**Interfaces:**
- Consumes tools: `youtube_plex_playlist`, `youtube_transfer_queue`
- Produces UI tabs: `search`, `stats`, `playlist`, `transfers`
- Produces host-safe link helpers:
  - `isAllowedExternalUrl(url)`
  - `openAllowedExternal(url)`
  - `copyText(text)`

- [ ] **Step 1: Write failing app tests**

Add to `src/search_app_tests.rs`:

```rust
#[test]
fn app_html_contains_playlist_and_transfers_views() {
    let html = crate::search_app::html();
    assert!(html.contains("data-view=\"playlist\""));
    assert!(html.contains("data-view=\"transfers\""));
    assert!(!html.contains("{{YOUTUBE_SEARCH_APP_SCRIPT}}"));
}

#[test]
fn app_metadata_allows_plex_external_destinations() {
    let meta = crate::search_app::resource_meta();
    let widget_csp = meta
        .get("openai/widgetCSP")
        .and_then(serde_json::Value::as_object)
        .unwrap();
    let redirects = widget_csp
        .get("redirect_domains")
        .and_then(serde_json::Value::as_array)
        .unwrap();
    assert!(redirects.iter().any(|value| value == "https://listen.plex.tv"));
    assert!(redirects.iter().any(|value| value == "https://app.plex.tv"));
}
```

- [ ] **Step 2: Run tests to verify failure**

```bash
cargo test search_app -- --nocapture
```

Expected: FAIL until views and metadata are added.

- [ ] **Step 3: Add tabs and views to HTML**

In `assets/youtube-search-app.html`, keep existing Search and Stats controls and add:

```html
<button class="tab" type="button" data-tab="playlist">Playlist</button>
<button class="tab" type="button" data-tab="transfers">Transfers</button>
```

Add view containers:

```html
<section class="view" data-view="playlist" hidden>
  <div class="toolbar">
    <input id="playlist-name" type="text" placeholder="Playlist name" />
    <button id="playlist-refresh" type="button">Refresh</button>
    <button id="playlist-preview" type="button">Preview</button>
    <button id="playlist-apply" type="button">Apply</button>
  </div>
  <div id="playlist-status" class="status"></div>
  <div id="playlist-results" class="results"></div>
</section>

<section class="view" data-view="transfers" hidden>
  <div class="toolbar">
    <button id="transfers-refresh" type="button">Refresh</button>
    <button id="transfers-retry-all" type="button">Retry all</button>
    <button id="transfers-prune" type="button">Prune missing</button>
  </div>
  <div id="transfers-status" class="status"></div>
  <div id="transfers-results" class="results"></div>
</section>
```

- [ ] **Step 4: Add safe JS helpers**

In `assets/youtube-search-app.js`, add:

```javascript
const ALLOWED_EXTERNAL_ORIGINS = new Set([
  "https://listen.plex.tv",
  "https://app.plex.tv",
]);

function isAllowedExternalUrl(url) {
  try {
    const parsed = new URL(url);
    return parsed.protocol === "https:" && ALLOWED_EXTERNAL_ORIGINS.has(parsed.origin);
  } catch {
    return false;
  }
}

async function openAllowedExternal(url) {
  if (!isAllowedExternalUrl(url)) {
    throw new Error("Blocked external link destination.");
  }
  return openExternal(url);
}

async function copyText(text) {
  if (!text) return false;
  if (navigator.clipboard && typeof navigator.clipboard.writeText === "function") {
    await navigator.clipboard.writeText(text);
    return true;
  }
  const area = document.createElement("textarea");
  area.value = text;
  area.setAttribute("readonly", "");
  area.style.position = "fixed";
  area.style.opacity = "0";
  document.body.appendChild(area);
  area.select();
  const ok = document.execCommand("copy");
  area.remove();
  return ok;
}
```

- [ ] **Step 5: Add Playlist handlers**

Add functions:

```javascript
async function loadPlaylistCandidates() {
  setStatus("playlist", "Loading candidates...");
  const payload = await callTool("youtube_plex_playlist", {
    action: "list_candidates",
    limit: 100,
    response_format: "json",
  });
  state.playlistCandidates = payload.candidates || [];
  renderPlaylistCandidates(payload);
  setStatus("playlist", `${state.playlistCandidates.length} candidate(s).`);
}

async function previewPlaylist() {
  const playlist = document.querySelector("#playlist-name").value.trim() || undefined;
  const candidate_ids = selectedCandidateIds();
  setStatus("playlist", "Previewing Plex matches...");
  const payload = await callTool("youtube_plex_playlist", {
    action: "preview",
    playlist,
    candidate_ids,
    limit: 100,
    response_format: "json",
  });
  renderPlaylistResult(payload, false);
  setStatus("playlist", "Preview complete.");
}

async function applyPlaylist() {
  const playlist = document.querySelector("#playlist-name").value.trim() || undefined;
  const candidate_ids = selectedCandidateIds();
  setStatus("playlist", "Applying Plex playlist...");
  const payload = await callTool("youtube_plex_playlist", {
    action: "apply",
    playlist,
    candidate_ids,
    limit: 100,
    response_format: "json",
  });
  renderPlaylistResult(payload, true);
  setStatus("playlist", "Playlist updated.");
}
```

Render candidates with checkboxes using `escapeHtml` for every dynamic field. Render `plexamp_url` with `Open in Plexamp` and `Copy link` buttons that call `openAllowedExternal` and `copyText`.

- [ ] **Step 6: Add Transfer handlers**

Add functions:

```javascript
async function loadTransfers() {
  setStatus("transfers", "Loading transfer queue...");
  const payload = await callTool("youtube_transfer_queue", {
    action: "list",
    response_format: "json",
  });
  renderTransfers(payload);
  setStatus("transfers", `${(payload.entries || []).length} queued transfer(s).`);
}

async function retryTransfer(manifest_id) {
  setStatus("transfers", "Retrying transfer...");
  await callTool("youtube_transfer_queue", {
    action: "retry",
    manifest_id,
    response_format: "json",
  });
  await loadTransfers();
}

async function retryAllTransfers() {
  setStatus("transfers", "Retrying all transfers...");
  await callTool("youtube_transfer_queue", {
    action: "retry_all",
    response_format: "json",
  });
  await loadTransfers();
}

async function pruneTransfers() {
  setStatus("transfers", "Pruning missing staging dirs...");
  await callTool("youtube_transfer_queue", {
    action: "prune",
    response_format: "json",
  });
  await loadTransfers();
}
```

- [ ] **Step 7: Update metadata**

In `src/search_app.rs`, add `https://listen.plex.tv` and `https://app.plex.tv` to:

- `_meta.ui.csp.resourceDomains` if used for anchors/assets
- `_meta.ui.csp.connectDomains` only if the UI fetches them directly; it should not
- `openai/widgetCSP.redirect_domains`

Mark `youtube_plex_playlist` and `youtube_transfer_queue` app-callable in `src/mcp.rs` with the same metadata helper as existing app-callable tools.

- [ ] **Step 8: Run app verification**

```bash
cargo test search_app mcp -- --nocapture
node --check assets/youtube-search-app.js
```

Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add assets/youtube-search-app.html assets/youtube-search-app.js src/search_app.rs src/search_app_tests.rs src/mcp.rs src/mcp_tests.rs
git commit -m "feat: add playlist and transfer app views"
```

### Task 5: Documentation And Packaging Sync

**Files:**
- Modify: `README.md`
- Modify: `packages/ytdl-rmcp/README.md`
- Modify: `openwiki/workflows/download-flow.md`
- Modify: `openwiki/architecture/overview.md`
- Modify: `CHANGELOG.md`
- Modify as needed if new env vars were added: `.claude-plugin/plugin.json`, `.mcp.json`, `mcpb/manifest.json`, `gemini-extension.json`, `scripts/check-packaging.sh`

**Interfaces:**
- Documents tools:
  - `youtube_plex_playlist`
  - `youtube_transfer_queue`
- Documents trust levels:
  - Official Plex PMS API
  - Community-proven Plexamp deep link
  - Local server-created manifest invariant

- [ ] **Step 1: Update README tool docs**

Add sections under the tools list:

```markdown
### `youtube_plex_playlist`

Build or preview Plex audio playlists from successful ytdl-rmcp download history.

Actions:

| Action | Meaning |
| --- | --- |
| `list_candidates` | Return audio candidates from history entries where `transferred` is `true`. |
| `preview` | Resolve selected candidates against Plex without mutating Plex. |
| `apply` | Add selected candidates to a Plex audio playlist idempotently. |

Candidates are history-derived and audio-only. Failed or retained-staging transfers are intentionally excluded.

`apply` can return `plexamp_url`, `plex_web_url`, and `playback_link_status`.
`plexamp_url` is a best-effort generated `listen.plex.tv` playback link, not an official Plexamp API guarantee.

### `youtube_transfer_queue`

List and drain server-created transfer failure manifests.

Actions:

| Action | Meaning |
| --- | --- |
| `list` | Show pending retained-staging transfer manifests. |
| `retry` | Retry one manifest by opaque `manifest_id`. |
| `retry_all` | Retry all pending manifests. |
| `prune` | Remove manifests whose staging directory is gone. |

The queue never accepts arbitrary filesystem paths. Retry uses the original target paths recorded at failure time and re-checks local target policy before transfer.
```

- [ ] **Step 2: Mirror package README and OpenWiki**

Mirror concise versions of the same content in `packages/ytdl-rmcp/README.md` and `openwiki/workflows/download-flow.md`.

- [ ] **Step 3: Update architecture overview**

Add bullets to `openwiki/architecture/overview.md`:

```markdown
- `history/candidates.rs` projects successful audio history entries into stable Plex playlist candidates.
- `plex/playlist.rs` shares the Plex preview/apply resolver and generates best-effort Plexamp/Plex Web links.
- `transfer_queue.rs` persists server-created transfer failure manifests and drains them by opaque ID only.
```

- [ ] **Step 4: Update CHANGELOG**

Add under Unreleased:

```markdown
- Added a Plex playlist builder for successful transferred audio history, with read-only preview, idempotent apply, and best-effort Plexamp/Plex Web links.
- Added a transfer queue for retained staging directories after transfer failures, with list/retry/retry-all/prune actions.
- Extended the MCP app with Playlist and Transfers tabs.
```

- [ ] **Step 5: Run packaging check**

```bash
scripts/check-packaging.sh
```

Expected: PASS. If this fails because new env vars were added, update every packaging surface named by the script and rerun.

- [ ] **Step 6: Commit**

```bash
git add README.md packages/ytdl-rmcp/README.md openwiki/workflows/download-flow.md openwiki/architecture/overview.md CHANGELOG.md .claude-plugin/plugin.json .mcp.json mcpb/manifest.json gemini-extension.json scripts/check-packaging.sh
git commit -m "docs: document playlist builder and transfer queue"
```

### Task 6: Full Verification And Review Prep

**Files:**
- Modify only if verification exposes issues.

**Interfaces:**
- Consumes all tasks.
- Produces a green branch ready for review and PR creation.

- [ ] **Step 1: Run formatting**

```bash
cargo fmt --all --check
```

Expected: PASS. If it fails, run `cargo fmt --all`, inspect diff, commit formatting with the relevant fix commit.

- [ ] **Step 2: Run full tests**

```bash
cargo test
```

Expected: PASS.

- [ ] **Step 3: Run clippy**

```bash
cargo clippy --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 4: Run packaging check**

```bash
scripts/check-packaging.sh
```

Expected: PASS.

- [ ] **Step 5: Run JS syntax check**

```bash
node --check assets/youtube-search-app.js
```

Expected: PASS.

- [ ] **Step 6: Optional UI screenshot smoke**

If Playwright/webwright is available in the worktree, capture desktop and mobile screenshots for Search, Stats, Playlist, and Transfers. Validate:

- no blank page
- no overlapping tab labels
- empty states render
- Plexamp buttons are present only when a safe generated link exists

- [ ] **Step 7: Check git status**

```bash
git status --short
```

Expected: clean except intentionally ignored local artifacts.

- [ ] **Step 8: Final commit if needed**

If verification fixes changed files:

```bash
git add .
git commit -m "test: verify playlist builder and transfer queue"
```

## Self-Review

- Spec coverage: covered successful audio history candidates, Plex preview/apply, Plexamp links, transfer manifests/drain, MCP app tabs, docs/packaging, and verification.
- Placeholder scan: no TODO/TBD placeholders; retry implementation step specifies required validation behavior and accepted tool path.
- Type consistency: `PlexPlaylistInput`, `PlexPlaylistAction`, `TransferQueueInput`, `TransferQueueAction`, `PlaylistCandidate`, and queue DTO names are consistent across tasks.
- Risk coverage: security research findings are covered by fail-closed queue locks, redacted transfer errors, no arbitrary paths, re-parse target policy, link allowlists, and token-free link generation.
