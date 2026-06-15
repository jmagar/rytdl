//! Persistent download ledger and stats derived from it.

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;

use anyhow::{Context, Result};
use chrono::{SecondsFormat, Utc};
use fs2::FileExt;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::bootstrap;
use crate::config::Config;
use crate::model::DownloadMode;
use crate::service::DownloadPayload;

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

/// Cap on retained ledger entries. Rotation keeps the most recent
/// `MAX_HISTORY_ENTRIES` lines and drops older ones. This bounds both the
/// on-disk size and the cost of the `stats_payload` full-file scan (which is
/// O(lines)). At ~1 KiB/line this caps the ledger near ~10 MiB.
const MAX_HISTORY_ENTRIES: usize = 10_000;

/// Rotation is amortized: we only scan/rewrite the file once the line count
/// drifts a bit past the cap, rather than on every single append.
const ROTATE_TRIGGER_ENTRIES: usize = MAX_HISTORY_ENTRIES + MAX_HISTORY_ENTRIES / 10;

pub(crate) fn append_download(
    cfg: &Config,
    mode: DownloadMode,
    payload: &DownloadPayload,
) -> Result<()> {
    let path = history_path(cfg);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create history directory {}", parent.display()))?;
    }

    // Persist the same ledger fields as before. Reading them off the typed
    // `DownloadPayload` (rather than by string key) means a field rename is now
    // a compile error here instead of a silently-`null` JSONL column. The
    // emitted keys/shape are unchanged.
    let entry = json!({
        "timestamp": timestamp_now(),
        "mode": mode.as_str(),
        "remote": payload.remote,
        "dest_path": payload.dest_path,
        "destination": payload.destination,
        "destinations": payload.destinations,
        "transferred": payload.transferred,
        "transfer_error": payload.transfer_error,
        "total_files": payload.total_files,
        "total_bytes": payload.total_bytes,
        "total_size": payload.total_size,
        "partial_items": payload.partial_items,
        "failed_items": payload.failed_items,
        "items": payload.items,
    });

    let line = serde_json::to_string(&entry)?;

    // Serialize the append+rotate critical section across threads AND processes
    // with an exclusive advisory lock on a sibling `.lock` file. Without it, two
    // concurrent appends can interleave inside a single O_APPEND `writeln!`
    // (write_all is not guaranteed to be one syscall) and a concurrent rotation
    // can rename a snapshot over the ledger between another writer's read and its
    // own append — corrupting JSONL or losing entries past the cap. Best-effort:
    // on a lock-less filesystem we warn and proceed (mirrors bootstrap's lock).
    let _guard = HistoryLock::acquire(&path);

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("open history file {}", path.display()))?;
    writeln!(file, "{line}").with_context(|| format!("write history file {}", path.display()))?;
    drop(file);

    // Bound growth best-effort. Rotation must NEVER fail a download: mirror the
    // append side-channel convention and only log to stderr on error.
    if let Err(error) = rotate_if_needed(&path) {
        tracing::warn!(%error, "failed to rotate download history; continuing");
    }
    Ok(())
}

/// Exclusive advisory lock over the append+rotate critical section, held for the
/// lifetime of the guard and released on drop. Uses a sibling `<ledger>.lock`
/// file so it never interferes with the ledger's own inode (which rotation
/// replaces via rename). Lock acquisition is best-effort: if the lock file can't
/// be created or locked (e.g. a filesystem without advisory locks), we warn once
/// and proceed unlocked rather than failing the download.
struct HistoryLock(Option<std::fs::File>);

impl HistoryLock {
    fn acquire(ledger: &Path) -> Self {
        let mut name = ledger
            .file_name()
            .map(|n| n.to_os_string())
            .unwrap_or_else(|| OsString::from("downloads.jsonl"));
        name.push(".lock");
        let lock_path = match ledger.parent() {
            Some(parent) => parent.join(name),
            None => PathBuf::from(name),
        };
        match OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
        {
            Ok(file) => match file.lock_exclusive() {
                Ok(()) => HistoryLock(Some(file)),
                Err(error) => {
                    tracing::warn!(%error, "could not lock history ledger; proceeding unlocked");
                    HistoryLock(None)
                }
            },
            Err(error) => {
                tracing::warn!(%error, "could not open history lock file; proceeding unlocked");
                HistoryLock(None)
            }
        }
    }
}

impl Drop for HistoryLock {
    fn drop(&mut self) {
        if let Some(file) = &self.0 {
            let _ = FileExt::unlock(file);
        }
    }
}

/// Process-local rotation counter. Combined with the PID it yields a temp
/// filename unique to each rotation attempt within this process, so two
/// concurrent `append_download` calls that both cross the trigger never write to
/// the same temp file (which would interleave/corrupt the JSONL). See
/// [`rotation_temp_path`].
static ROTATION_SEQ: AtomicU64 = AtomicU64::new(0);

/// Build a per-attempt temp path *in the same directory* as the ledger (so the
/// final `rename` is atomic — same filesystem). The name embeds the PID and a
/// process-local atomic counter rather than a fixed `downloads.jsonl.tmp`, so
/// concurrent rotations get distinct temp files and cannot clobber each other.
fn rotation_temp_path(path: &std::path::Path) -> PathBuf {
    let seq = ROTATION_SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    // Suffix the existing file name so the temp sits beside the ledger and keeps
    // its directory (e.g. `downloads.jsonl` -> `downloads.jsonl.<pid>.<seq>.tmp`).
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_else(|| OsString::from("downloads.jsonl"));
    name.push(format!(".{pid}.{seq}.tmp"));
    match path.parent() {
        Some(parent) => parent.join(name),
        None => PathBuf::from(name),
    }
}

/// Trim the ledger to the last [`MAX_HISTORY_ENTRIES`] lines if it has grown
/// past [`ROTATE_TRIGGER_ENTRIES`]. Rewrites via a sibling temp file + rename so
/// a crash mid-rotation cannot corrupt or truncate the live ledger (the rename
/// is atomic; on failure the original file is left untouched).
///
/// Concurrency: the temp file is unique per attempt (see [`rotation_temp_path`]),
/// so concurrent rotations never interleave writes. The `rename`s may still race,
/// in which case the last writer wins — both inputs are valid trimmed snapshots,
/// so that is acceptable. Each rotation cleans up its own temp file on error.
fn rotate_if_needed(path: &std::path::Path) -> Result<()> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error).with_context(|| format!("open history file {}", path.display()))
        }
    };

    // Keep only the last MAX_HISTORY_ENTRIES lines in a bounded ring buffer so
    // we never hold the whole (over-cap) file in memory at once.
    //
    // A single unreadable line (e.g. invalid UTF-8 from a prior corrupted write)
    // must NOT wedge rotation forever: instead of `?`-aborting on a read error we
    // drop that line and keep going, so the poison line gets rotated OUT instead
    // of growing the ledger unbounded. Dropped lines are counted and warned once
    // below. A dropped line still counts toward `total` (it occupies a physical
    // line and contributes to file growth), keeping the trigger logic correct.
    let mut tail: VecDeque<String> = VecDeque::with_capacity(MAX_HISTORY_ENTRIES + 1);
    let mut total = 0_usize;
    let mut dropped = 0_u64;
    let mut lines = BufReader::new(file).lines();
    loop {
        match lines.next() {
            Some(Ok(line)) => {
                total += 1;
                tail.push_back(line);
                if tail.len() > MAX_HISTORY_ENTRIES {
                    tail.pop_front();
                }
            }
            Some(Err(_)) => {
                // Unreadable line: count it as a dropped entry and continue, so a
                // poison line is rotated out rather than aborting the rotation.
                total += 1;
                dropped += 1;
            }
            None => break,
        }
    }

    if dropped > 0 {
        tracing::warn!(
            dropped,
            "dropped unreadable lines while rotating download history"
        );
    }

    if total <= ROTATE_TRIGGER_ENTRIES {
        return Ok(());
    }

    let tmp = rotation_temp_path(path);
    if let Err(error) = write_and_swap(&tmp, path, &tail) {
        // Best-effort cleanup so a failed rotation doesn't orphan our temp file.
        let _ = std::fs::remove_file(&tmp);
        return Err(error);
    }
    Ok(())
}

/// Write the trimmed `tail` to `tmp`, then atomically `rename` it over `path`.
/// Split out so the caller can clean up `tmp` on any error from here.
fn write_and_swap(
    tmp: &std::path::Path,
    path: &std::path::Path,
    tail: &VecDeque<String>,
) -> Result<()> {
    {
        let mut out = std::fs::File::create(tmp)
            .with_context(|| format!("create temp history file {}", tmp.display()))?;
        for line in tail {
            writeln!(out, "{line}")
                .with_context(|| format!("write temp history file {}", tmp.display()))?;
        }
        out.flush()
            .with_context(|| format!("flush temp history file {}", tmp.display()))?;
    }
    std::fs::rename(tmp, path)
        .with_context(|| format!("replace history file {}", path.display()))?;
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
    // `downloads` and `calls` were historically two fields that `add_call`
    // always incremented together, so they were identical by construction.
    // Collapsed to a single counter to remove the redundancy. The stats JSON
    // still exposes both `downloads` and `calls` keys (sourced from this one
    // field) because the README documents `downloads` as a compatibility alias
    // for the call count — see `buckets_to_value`.
    calls: u64,
    items: u64,
    files: u64,
    bytes: u64,
}

impl Bucket {
    fn add_call(&mut self) {
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
                        // `downloads` is a documented compatibility alias for
                        // the call count; both keys are sourced from the single
                        // `calls` field (see the `Bucket` definition).
                        "downloads": bucket.calls,
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
