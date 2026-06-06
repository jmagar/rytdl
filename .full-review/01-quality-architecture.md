# Phase 1: Code Quality and Architecture

## Findings

- High - `src/downloader.rs:198` and `src/service.rs:300`
  The `both` download path can produce partial success that the result model cannot represent cleanly. `downloader::fetch` runs video first, appends files to the same `ItemResult`, then sets `result.error` and returns if the audio pass fails. `run_download` still transfers any produced files because `total_files > 0`, but the markdown renderer treats any item-level error as a total item failure and suppresses the file list.
  Impact: users can see a top-level "Transferred N file(s)" message followed by an item-level failure, with no clear indication that the video succeeded and audio failed. Automation consuming JSON also has to infer partial state from the conflicting `error` plus `files` fields.
  Fix: represent per-pass outcomes explicitly, for example `ItemResult { files, errors: Vec<PassError> }` or separate audio/video child results. Render partial success as partial success, and make transfer/reporting consume the same structured state.

- Medium - `src/model.rs:128`, `src/model.rs:131`, `src/model.rs:134`, and `src/service.rs:22`
  Remote transfer configuration is modeled as plain optional strings all the way from MCP input into orchestration. The service has no single validation or normalization boundary for `remote`, `dest_path`, and `video_dest_path` before invoking external transfer code.
  Impact: architecture leaves command-safety, path policy, and user-facing validation scattered across subprocess call sites. This is already visible in Phase 2 as command-boundary risk.
  Fix: introduce a small validated transfer config type, for example `TransferTarget { remote: RemoteSpec, audio_dest: RemotePath, video_dest: RemotePath }`, constructed once in `run_download` before downloads start.

- Medium - `src/config.rs:46` and `src/transfer.rs:55`
  `YTDLP_SSH_OPTS` is parsed with `split_whitespace()` and later converted back into a single `ssh` command string for rsync. This makes option parsing lossy and ties config semantics to a local shell-style string even though the rest of the code generally uses argv-safe `Command` APIs.
  Impact: quoted options or values containing spaces cannot be represented reliably. It also makes the transfer module harder to reason about because the same logical option list is handled as argv in `ssh`/`scp` and as a command string in `rsync`.
  Fix: avoid free-form extra SSH option strings if possible. Prefer structured config fields for common needs such as port, identity file, known-hosts behavior, and proxy jump. If arbitrary options stay, parse with shell-word semantics and build the rsync remote-shell command through a tested quoting helper.

- Low - `src/service.rs:22`
  `run_download` handles configuration resolution, tool bootstrapping, staging setup, download sequencing, transfer sequencing, cleanup decisions, payload construction, and markdown rendering coordination in one path.
  Impact: the function is still under the repo's size limit, but it has accumulated several independent responsibilities. This increases the chance that future changes to reporting, transfer, or staging will affect each other.
  Fix: split into small helpers for `resolve_download_plan`, `download_all`, `transfer_outputs`, and `cleanup_staging`, while preserving the existing module boundaries.

## Positive Notes

- Module boundaries are clear and match the documented architecture.
- Files stay under the 500-line convention.
- Tests are in sibling `foo_tests.rs` files as required.
- Logging is routed to stderr in `src/main.rs`, preserving stdout for MCP JSON-RPC.
- `AGENTS.md` and `GEMINI.md` are symlinks to `CLAUDE.md`.

## Verification

- `cargo fmt --all --check` - passed.
- `cargo test --all` - passed; 15 tests passed.
- `cargo clippy --all-targets -- -D warnings` - passed.

## Critical Issues for Phase 2 Context

- The transfer target strings have no validation boundary before crossing SSH, rsync, and scp subprocess boundaries.
- Partial success in `mode = both` creates conflicting result state, which can hide what was actually downloaded and transferred.
