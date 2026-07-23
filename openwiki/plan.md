---
type: Reference
title: "Docs update plan"
description: "Draft plan for this update run and source-evidence mapping"
---

## Quick impact assessment

- Last run recorded in `openwiki/.last-update.json` points to `c30ec37e70c644dff9c26b1b0428370d5150b41d`.
- Current HEAD includes a massive synthetic history import, but this update target has a narrow scope: workflow dependency bumps and `openwiki` files modified by maintainers after `c30ec...`.
- Focused docs updates should correct stale statements about config defaults, tool coverage, and workflow references introduced by recent code evolution and ensure backlogged areas are tracked.

## Evidence to validate

- Source changes to inspect:
  - `src/config.rs`
  - `src/setup.rs`
  - `src/model.rs`
  - `src/identify.rs`
  - `src/bootstrap.rs`
  - `.github/workflows/*.yml`
- Wiki files to adjust:
  - `openwiki/quickstart.md`
  - `openwiki/architecture/overview.md`
  - `openwiki/workflows/download-flow.md`
  - `openwiki/operations/setup.md`
  - `openwiki/operations/container.md`
  - `openwiki/development/build-test.md`

## Source-driven deltas discovered

- `src/config.rs` now exposes `Config` parsing for `YTDLP_*`, plus target migration helpers and timeout/hash validation.
- `src/setup.rs` documents tool installation path and agent registration behavior for claude/codex/gemini.
- `src/model.rs` shows `youtube_plex_playlist` and `youtube_transfer_queue` inputs are part of active model surface.
- `.github/workflows/*.yml` diffs reflect dependency bumps for actions/toolchain actions; no behavior changes in product flow.
- `src/identify.rs` includes fpcalc `--` guarding and retag preview lookup and write flow.

## Planned docs edits

1. Update `openwiki/quickstart.md`
   - Keep tool list accurate with current surfaces.
   - Verify setup and config references align with `src/config.rs`.
2. Update `openwiki/architecture/overview.md`
   - Confirm module layout mentions model/setup/bootstrap links.
   - Ensure tool list covers eight tools and current security notes.
3. Update `openwiki/operations/setup.md`
   - Align required/optional env list to implementation in `Config::from_env_result`.
   - Remove stale remote-path defaults and note newer `target_path` guidance.
4. Update `openwiki/operations/container.md` where needed for current tool bootstrap semantics.
5. Update `openwiki/workflows/download-flow.md` if identifiers/order changed after model/config updates.
6. Update `openwiki/development/build-test.md` only where CI workflow names/roles changed.

## TODO graph

- [ ] Validate all references: source commit list and changed file list
- [ ] Keep `openwiki/_plan.md` with front matter and remove after edits
- [ ] Run a final `git status` check and ensure `openwiki/_plan.md` is removed
- [ ] Add/update `openwiki/.last-update.json` with current commit and timestamp

## Watch-outs

- `src/model.rs` and setup flow include additional tool surfaces not reflected in some prose.
- Legacy `YTDLP_REMOTE(_PATH)` still used only for migration compatibility in config; primary path is now `target_path`.
- Keep `quickstart` claims conservative unless directly evidenced by source/test.
