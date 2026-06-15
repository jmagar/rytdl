//! Auto-retag orchestration extracted from `service.rs`.
//!
//! After a download completes, if AcoustID is configured we fingerprint the
//! freshly downloaded audio files and write MusicBrainz-derived tags back. The
//! per-run outcome is summarized into the typed [`RetagSummary`] which the
//! download payload carries on its `metadata_retag` side-channel.
//!
//! The identify step is injected as a closure (`auto_retag_audio_paths` is
//! generic over it) so tests can drive the summary logic without a live
//! AcoustID/MusicBrainz round-trip.

use std::future::Future;
use std::sync::Arc;

use anyhow::Result;
use serde::Serialize;

use crate::config::Config;
use crate::downloader::ItemResult;
use crate::identify::IdentifyPayload;

/// Typed summary of an auto-retag run, carried on
/// [`crate::service::DownloadPayload::metadata_retag`].
///
/// The serialized JSON is byte-compatible with the previous hand-built `json!`
/// maps. Two shapes are produced from the same struct via `skip_serializing_if`:
///
/// * success: `{attempted, matched, written, skipped, errors}` — `skipped` is
///   `Some`, `error` is `None`.
/// * identify failure: `{attempted, matched, written, errors, error}` —
///   `skipped` is `None`, `error` is `Some`.
///
/// Field declaration order matches both legacy maps because the only field that
/// differs between them (`skipped`) sits where the success map had it, and the
/// failure-only `error` field trails after `errors`.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct RetagSummary {
    pub attempted: usize,
    pub matched: usize,
    pub written: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped: Option<usize>,
    pub errors: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub(super) async fn auto_retag_audio(
    cfg: &Arc<Config>,
    results: &[ItemResult],
) -> Option<RetagSummary> {
    let paths = downloaded_audio_paths(results);
    auto_retag_audio_paths(cfg, paths, |cfg, paths| async move {
        crate::identify::identify_files(&cfg, paths, true).await
    })
    .await
}

fn downloaded_audio_paths(results: &[ItemResult]) -> Vec<String> {
    results
        .iter()
        .flat_map(|result| &result.files)
        .filter(|file| file.kind == "audio")
        .map(|file| file.path.display().to_string())
        .collect()
}

async fn auto_retag_audio_paths<F, Fut>(
    cfg: &Arc<Config>,
    paths: Vec<String>,
    identify: F,
) -> Option<RetagSummary>
where
    F: FnOnce(Arc<Config>, Vec<String>) -> Fut,
    Fut: Future<Output = Result<IdentifyPayload>>,
{
    if paths.is_empty()
        || cfg
            .acoustid_client_key
            .as_deref()
            .filter(|key| !key.trim().is_empty())
            .is_none()
    {
        return None;
    }

    let attempted = paths.len();
    match identify(Arc::clone(cfg), paths).await {
        Ok(payload) => Some(auto_retag_summary(&payload, attempted)),
        Err(error) => Some(RetagSummary {
            attempted,
            matched: 0,
            written: 0,
            skipped: None,
            errors: 1,
            error: Some(error.to_string()),
        }),
    }
}

#[cfg(test)]
pub(crate) async fn auto_retag_audio_paths_for_test<F, Fut>(
    cfg: &Arc<Config>,
    paths: Vec<String>,
    identify: F,
) -> Option<serde_json::Value>
where
    F: FnOnce(Arc<Config>, Vec<String>) -> Fut,
    Fut: Future<Output = Result<IdentifyPayload>>,
{
    auto_retag_audio_paths(cfg, paths, identify)
        .await
        .map(|summary| serde_json::to_value(summary).expect("retag summary serializes"))
}

fn auto_retag_summary(payload: &IdentifyPayload, attempted: usize) -> RetagSummary {
    let matched = payload
        .results
        .iter()
        .filter(|result| result.retag_preview.is_some())
        .count();
    let written = payload
        .results
        .iter()
        .filter(|result| {
            result
                .tag_write
                .as_ref()
                .map(|write| write.written)
                .unwrap_or(false)
        })
        .count();
    let errors = payload
        .results
        .iter()
        .filter(|result| {
            result.error.is_some()
                || result.retag_preview_error.is_some()
                || result.tag_write_error.is_some()
        })
        .count();

    RetagSummary {
        attempted,
        matched,
        written,
        skipped: Some(attempted.saturating_sub(matched)),
        errors,
        error: None,
    }
}
