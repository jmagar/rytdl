# Phase 3: Testing and Documentation

## Post-Review Resolution

- Resolved - `src/transfer.rs` and `src/transfer_tests.rs`
  After this phase was written, transfer subprocesses were routed through a helper that sets `kill_on_drop(true)`, and `transfer::tests::dropped_transfer_command_kills_child_process` was added to prove a timed-out transfer future does not leave a sleeping child alive. `cargo test --all` now passes with 30 tests.
- Resolved - `src/config.rs`, `src/config_tests.rs`, `src/bootstrap_tests.rs`, and `src/service_tests.rs`
  Fallible config loading now reports invalid SHA pins, malformed SSH option shell words, and invalid or zero timeout values. Additional tests cover env-to-config wiring, SHA enforcement through `ensure_ytdlp`/`ensure`, transfer validation before tool resolution, and fake-runtime JSON partial status. `cargo test --all` now passes with 38 tests.
- Resolved - `skills/ytdl/SKILL.md`, `README.md`, `.claude-plugin/plugin.json`, `.mcp.json`, and `gemini-extension.json`
  The skill and user-facing docs now describe ffmpeg/yt-dlp auto-resolution, optional rsync with scp fallback, shell-word SSH options, timeout controls, checksum pins, binary overrides, and extractor args.

## Original Findings (Resolved)

- Resolved High - `src/service.rs:131`, `src/transfer.rs:98`, `src/transfer.rs:137`, and `src/transfer.rs:166`
  Transfer timeout behavior is not covered by a test that proves the spawned `ssh`, `rsync`, or `scp` process is terminated on timeout. The current fix wraps `transfer_kind(...)` in `tokio::time::timeout`, but the transfer helpers still use `Command::output()` directly and do not set `kill_on_drop(true)`. Tokio's local docs state that spawned processes continue by default after the `Child` handle is dropped unless `kill_on_drop` is enabled.
  Impact: `YTDLP_TRANSFER_TIMEOUT_SECS` can return an error to the MCP caller while leaving a live SSH-family child process behind, so the operational timeout guarantee is incomplete.
  Fix: move transfer command execution through a shared async command helper that sets `kill_on_drop(true)`, kills and awaits the child on timeout, captures bounded stderr, and add a test with a fake/sleeping command that proves no child remains.

- Resolved Medium - `src/config.rs:104` and `src/config_tests.rs:69`
  `YTDLP_SSH_OPTS` now uses `shell_words::split`, which is the right direction for quoted options, but malformed quoting is silently converted into an empty option list through `unwrap_or_default()`. There is also no `Config::from_env` test proving invalid input is surfaced or at least reported.
  Impact: a user can configure a broken identity file or proxy option and unknowingly run transfers without those options, making failures harder to diagnose and weakening the reliability of the transfer boundary.
  Fix: make SSH option parsing fallible at configuration load time, return a clear error from server startup/setup where possible, or log a warning and preserve an explicit "ignored invalid options" state. Add tests for unmatched quotes and escaped values.

- Resolved Medium - `src/service_tests.rs:14`, `src/transfer_tests.rs:4`, and `src/bootstrap_tests.rs:25`
  The new tests cover pure helpers well, but they do not exercise the user-facing MCP/tool layer, env-to-config wiring, or the download/transfer orchestration boundary. In particular, there is no integration test for `youtube_download` JSON output with partial status, no fixture-based test proving `TransferTarget::parse` rejects tool-input `remote`/destination values before any download starts, and no test that SHA-256 pins are enforced through `ensure_ytdlp`/`ensure`.
  Impact: regressions can pass unit tests while breaking the actual rmcp tool contract or the env-driven plugin/runtime path users run.
  Fix: add fixture-oriented integration tests around `run_download` with fake `yt-dlp`/`ffmpeg`/`ssh` commands on `PATH`, plus direct rmcp schema/tool smoke where practical.

- Resolved Medium - `skills/ytdl/SKILL.md:72`
  The skill still says the host running the plugin requires `ffmpeg`, while `README.md:127` and the architecture state that ffmpeg is auto-downloaded unless overridden.
  Impact: agents following the skill can give users stale setup guidance and may waste time installing ffmpeg even though the binary is intended to be self-contained.
  Fix: update the skill requirements to match README: require SSH plus passwordless auth, note that rsync is optional with scp fallback, and describe yt-dlp/ffmpeg as auto-resolved with optional env overrides.

- Resolved Low - `README.md:104` and `.claude-plugin/plugin.json:45`
  `YTDLP_SSH_OPTS` is documented as "space-separated", but current parsing supports shell-style quoting through `shell_words`. The docs do not explain how to quote values with spaces or what happens on malformed quoting.
  Impact: users configuring `-i '/path/media key'` or proxy commands may not know the supported syntax, and malformed strings currently fail silently.
  Fix: document shell-word syntax with one quoted identity-file example, and align the plugin description with the README.

- Resolved Low - `.claude-plugin/plugin.json:86`, `.mcp.json:17`, `gemini-extension.json:38`, and `README.md:110`
  Timeout and checksum settings are exposed across Claude plugin, MCP env mapping, Gemini extension, and README, but the skill does not mention these operational controls.
  Impact: agent-guided usage can miss the most important knobs added by the remediation, especially for long playlists, slow remotes, and locked-down supply-chain operation.
  Fix: add a short "Operational controls" section to `skills/ytdl/SKILL.md` covering `YTDLP_TIMEOUT_SECS`, `YTDLP_TRANSFER_TIMEOUT_SECS`, `YTDLP_SHA256`, `FFMPEG_SHA256`, and `YTDLP_PATH`/`FFMPEG_PATH`.

## Positive Notes

- The newly added unit tests directly cover the core previously reported issues: remote mkdir shell quoting, remote validation, rsync remote-shell option quoting, partial-result rendering, timeout error reporting for the download helper, SHA-256 digest matching, and quoted SSH option parsing.
- `jq empty .claude-plugin/plugin.json .mcp.json gemini-extension.json hooks/hooks.json` passed, so the edited JSON/plugin artifacts are syntactically valid.
- `AGENTS.md` and `GEMINI.md` remain symlinks to `CLAUDE.md`, preserving the repo memory convention.

## Verification

- `sed -n '1,240p' .full-review/00-scope.md` - inspected prior scope.
- `sed -n '1,260p' .full-review/01-quality-architecture.md` - inspected Phase 1.
- `sed -n '1,280p' .full-review/02-security-performance.md` - inspected Phase 2.
- `git status --short --branch` - inspected current dirty state; product code/docs are modified and new test files are untracked.
- `git diff --stat && git diff --name-status` - inspected current tracked diff; untracked test files were separately read.
- `git diff -- src/transfer.rs src/service.rs src/downloader.rs src/config.rs` - inspected current product-code fixes.
- `git diff -- README.md CLAUDE.md .claude-plugin/plugin.json .mcp.json gemini-extension.json skills/ytdl/SKILL.md .github/workflows/ci.yml .github/workflows/release.yml` - inspected documentation/config diffs; no tracked skill or CI diff was present.
- `sed -n '1,260p' src/transfer_tests.rs && sed -n '1,260p' src/downloader_tests.rs && sed -n '1,260p' src/service_tests.rs` - inspected new untracked tests.
- `jq empty .claude-plugin/plugin.json .mcp.json gemini-extension.json hooks/hooks.json` - passed with no output.
- `cargo fmt --all --check` - passed with no output.
- `cargo metadata --no-deps --format-version 1` - passed and identified the single `ytdl-mcp` package.
- `cargo tree -i aws-lc-sys` - exited 101 with `error: package ID specification 'aws-lc-sys' did not match any packages`; interpreted as `aws-lc-sys` absent from the dependency tree.
- Coordinator/baseline test status: `.full-review/00-scope.md`, Phase 1, and Phase 2 all report `cargo test --all` passed with 15 tests before the current remediation. I did not find a separate current coordinator test artifact for the newly added tests, and I did not run `cargo test` because this turn was constrained to review-artifact writes only.
