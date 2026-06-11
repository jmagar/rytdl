//! Persistent download ledger and stats derived from it.

use anyhow::{Context, Result};
use chrono::{SecondsFormat, Utc};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::path::PathBuf;

use crate::bootstrap;
use crate::config::Config;
use crate::model::DownloadMode;

fn default_history_path() -> PathBuf {
    bootstrap::project_dirs()
        .map(|d| {
            d.state_dir()
                .unwrap_or_else(|| d.data_dir())
                .join("downloads.jsonl")
        })
        .unwrap_or_else(|| std::env::temp_dir().join("ytdl-mcp-state/downloads.jsonl"))
}

fn history_path(cfg: &Config) -> PathBuf {
    cfg.history_path
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(default_history_path)
}

pub(crate) fn append_download(cfg: &Config, mode: DownloadMode, payload: &Value) -> Result<()> {
    let path = history_path(cfg);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create history directory {}", parent.display()))?;
    }

    let entry = json!({
        "timestamp": timestamp_now(),
        "mode": mode.as_str(),
        "remote": payload["remote"].clone(),
        "dest_path": payload["dest_path"].clone(),
        "destination": payload["destination"].clone(),
        "destinations": payload["destinations"].clone(),
        "transferred": payload["transferred"].clone(),
        "transfer_error": payload["transfer_error"].clone(),
        "total_files": payload["total_files"].clone(),
        "total_bytes": payload["total_bytes"].clone(),
        "total_size": payload["total_size"].clone(),
        "partial_items": payload["partial_items"].clone(),
        "failed_items": payload["failed_items"].clone(),
        "items": payload["items"].clone(),
    });

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("open history file {}", path.display()))?;
    writeln!(file, "{}", serde_json::to_string(&entry)?)
        .with_context(|| format!("write history file {}", path.display()))?;
    Ok(())
}

pub(crate) fn stats_payload(cfg: &Config, limit: usize) -> Result<Value> {
    let path = history_path(cfg);
    let file = match std::fs::File::open(&path) {
        Ok(file) => Some(file),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => {
            return Err(error).with_context(|| format!("open history file {}", path.display()))
        }
    };

    let mut by_kind: BTreeMap<String, Bucket> = BTreeMap::new();
    let mut by_uploader: BTreeMap<String, Bucket> = BTreeMap::new();
    let mut recent: VecDeque<Value> = VecDeque::new();
    let mut total_downloads = 0_u64;
    let mut total_files = 0_u64;
    let mut total_bytes = 0_u64;
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
            total_downloads += 1;
            accumulate_entry(
                &entry,
                &mut by_kind,
                &mut by_uploader,
                &mut total_files,
                &mut total_bytes,
            );
            if limit > 0 {
                recent.push_front(entry);
                recent.truncate(limit);
            }
        }
    }

    Ok(json!({
        "history_path": path.display().to_string(),
        "total_downloads": total_downloads,
        "total_files": total_files,
        "total_bytes": total_bytes,
        "total_size": human_size(total_bytes),
        "skipped_entries": skipped_entries,
        "by_kind": buckets_to_value(by_kind),
        "by_uploader": buckets_to_value(by_uploader),
        "recent": recent.into_iter().collect::<Vec<_>>(),
    }))
}

fn accumulate_entry(
    entry: &Value,
    by_kind: &mut BTreeMap<String, Bucket>,
    by_uploader: &mut BTreeMap<String, Bucket>,
    total_files: &mut u64,
    total_bytes: &mut u64,
) {
    *total_files += entry["total_files"].as_u64().unwrap_or(0);
    *total_bytes += entry["total_bytes"].as_u64().unwrap_or(0);
    let mut entry_kinds = BTreeSet::new();
    let mut entry_uploaders = BTreeSet::new();

    for item in entry["items"].as_array().into_iter().flatten() {
        let mut item_kinds = BTreeSet::new();
        let uploader = item["uploader"].as_str().filter(|s| !s.is_empty());
        if let Some(uploader) = uploader {
            entry_uploaders.insert(uploader.to_string());
            by_uploader.entry(uploader.to_string()).or_default().items += 1;
        }

        for file in item["files"].as_array().into_iter().flatten() {
            let kind = file["kind"].as_str().unwrap_or("unknown").to_string();
            let bytes = file["bytes"].as_u64().unwrap_or(0);
            entry_kinds.insert(kind.clone());
            item_kinds.insert(kind.clone());
            by_kind.entry(kind).or_default().add_file(bytes);
            if let Some(uploader) = uploader {
                by_uploader
                    .entry(uploader.to_string())
                    .or_default()
                    .add_file(bytes);
            }
        }

        for kind in item_kinds {
            by_kind.entry(kind).or_default().items += 1;
        }
    }

    for kind in entry_kinds {
        by_kind.entry(kind).or_default().add_call();
    }
    for uploader in entry_uploaders {
        by_uploader.entry(uploader).or_default().add_call();
    }
}

pub(crate) fn render_stats_markdown(payload: &Value) -> String {
    let mut lines = vec![format!(
        "{} download(s), {} file(s), {} total.",
        payload["total_downloads"].as_u64().unwrap_or(0),
        payload["total_files"].as_u64().unwrap_or(0),
        payload["total_size"].as_str().unwrap_or("0 B")
    )];

    if let Some(kinds) = payload["by_kind"].as_object().filter(|m| !m.is_empty()) {
        lines.push(String::new());
        lines.push("By kind:".into());
        for (kind, bucket) in kinds {
            lines.push(format!(
                "- {kind}: {} file(s), {}",
                bucket["files"].as_u64().unwrap_or(0),
                bucket["size"].as_str().unwrap_or("0 B")
            ));
        }
    }

    if let Some(skipped) = payload["skipped_entries"].as_u64().filter(|n| *n > 0) {
        lines.push(String::new());
        lines.push(format!("Skipped {skipped} malformed history entries."));
    }

    if let Some(recent) = payload["recent"].as_array().filter(|a| !a.is_empty()) {
        lines.push(String::new());
        lines.push("Recent:".into());
        for entry in recent {
            let title = entry["items"]
                .as_array()
                .and_then(|items| items.first())
                .and_then(|item| item["title"].as_str())
                .unwrap_or("Untitled");
            lines.push(format!(
                "- {} - {} ({})",
                entry["timestamp"].as_str().unwrap_or("unknown time"),
                title,
                entry["total_size"].as_str().unwrap_or("0 B")
            ));
        }
    }

    lines.join("\n").trim().to_string()
}

fn timestamp_now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

#[derive(Default)]
struct Bucket {
    downloads: u64,
    calls: u64,
    items: u64,
    files: u64,
    bytes: u64,
}

impl Bucket {
    fn add_call(&mut self) {
        self.downloads += 1;
        self.calls += 1;
    }

    fn add_file(&mut self, bytes: u64) {
        self.files += 1;
        self.bytes += bytes;
    }
}

fn buckets_to_value(buckets: BTreeMap<String, Bucket>) -> Value {
    Value::Object(
        buckets
            .into_iter()
            .map(|(key, bucket)| {
                (
                    key,
                    json!({
                        "downloads": bucket.downloads,
                        "calls": bucket.calls,
                        "items": bucket.items,
                        "files": bucket.files,
                        "bytes": bucket.bytes,
                        "size": human_size(bucket.bytes),
                    }),
                )
            })
            .collect(),
    )
}

fn human_size(bytes: u64) -> String {
    let mut size = bytes as f64;
    for unit in ["B", "KiB", "MiB", "GiB", "TiB"] {
        if size < 1024.0 || unit == "TiB" {
            if unit == "B" {
                return format!("{bytes} B");
            }
            return format!("{size:.1} {unit}");
        }
        size /= 1024.0;
    }
    format!("{bytes} B")
}
