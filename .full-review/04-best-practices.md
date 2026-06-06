# Phase 4: Best Practices and Standards

## Post-Review Resolution

- Resolved - `src/transfer.rs` and `src/transfer_tests.rs`
  After this phase was written, the transfer path was updated to use a kill-on-drop command helper, and a Unix regression test now verifies dropped transfer command futures terminate the child process. `cargo fmt --all --check`, `cargo test --all`, and `cargo clippy --all-targets -- -D warnings` pass after the fix.
- Resolved - `src/config.rs`, `src/main.rs`, `src/setup.rs`, and tests
  Runtime startup and setup now use fallible config loading. Invalid checksum pins, malformed SSH option shell words, and invalid timeout values are reported instead of silently degrading.
- Resolved - `.github/workflows/ci.yml`, `.github/workflows/release.yml`, `scripts/check-packaging.sh`, and `scripts/fetch-binary.sh`
  CI/release now validate plugin packaging, JSON, shell scripts, ShellCheck, Claude userConfig mapping, and Gemini env mappings. Missing release checksums are hard failures unless `YTDL_MCP_ALLOW_MISSING_CHECKSUM=1` is set for compatibility testing.
- Resolved - `README.md` and `CLAUDE.md`
  Local dookie/xwin docs now call out `~/.cargo/bin/cargo xwin`, and edition 2021 is documented as intentional compatibility for the distributable cross-platform binary.

## Original Findings (Resolved)

- Resolved High - `src/service.rs:131` and `src/transfer.rs:98`
  The transfer timeout implementation does not follow the same process-management best practice as the new downloader helper. `src/downloader.rs:352` explicitly spawns with `kill_on_drop(true)` and kills the child on timeout; the transfer path wraps helpers that call `Command::output()` without setting kill-on-drop. Tokio's process docs explicitly warn that dropped children continue by default.
  Impact: the server advertises bounded transfer behavior but may leave runaway `ssh`, `rsync`, or `scp` processes after a timeout, which is a production reliability issue for a long-lived MCP server.
  Fix: centralize external command execution in one helper used by downloader and transfer modules. Require timeout, bounded stderr capture, `kill_on_drop(true)`, explicit `kill().await` on timeout, and consistent error text.

- Resolved Medium - `src/config.rs:49`
  `Config::from_env` cannot return errors, which pushes invalid operational configuration into silent fallback paths such as invalid SHA-256 pins becoming `None` and malformed `YTDLP_SSH_OPTS` becoming an empty list.
  Impact: production/runtime misconfiguration can silently weaken controls that were added specifically for security and reliability. A bad checksum pin should not behave like no pin, and broken SSH options should be visible immediately.
  Fix: introduce a fallible config loader, for example `Config::from_env() -> Result<Config>`, and let MCP startup/setup report invalid pins, invalid timeouts, and unparsable SSH options clearly.

- Resolved Medium - `.github/workflows/ci.yml:34` and `.github/workflows/release.yml:13`
  CI and release workflows build and test Linux plus Windows cross-compile, but they do not validate the packaged Claude plugin or Gemini extension behavior. The plugin installer has its own shell logic and checksum flow in `scripts/fetch-binary.sh`, and those paths are not exercised in CI.
  Impact: a release can pass Rust tests while shipping a broken plugin manifest, broken userConfig-to-env mapping, or a Linux plugin installer that fails to fetch/verify the release binary.
  Fix: add a CI job that validates plugin JSON, runs shellcheck on scripts, runs `scripts/fetch-binary.sh` against a controlled fixture or latest release in dry-run-style mode, and verifies `.mcp.json` references only declared `userConfig` keys.

- Resolved Medium - `.github/workflows/ci.yml:52`, `.github/workflows/release.yml:54`, and `CLAUDE.md:50`
  Local docs correctly warn that the user's `~/.local/bin/cargo` wrapper breaks `cargo xwin`, but CI and README examples still use plain `cargo xwin`. That is fine on GitHub-hosted runners, but the distinction is easy to miss for local release rehearsal on dookie.
  Impact: local cross-build verification can fail with misleading standard-library errors even while CI would pass, wasting release/debugging time.
  Fix: in README or CLAUDE, make the local command explicitly use `~/.cargo/bin/cargo xwin ...` for dookie/local workflows, while keeping CI unchanged.

- Resolved Low - `Cargo.toml:1`
  The crate is still on Rust edition 2021 while the broader local Rust workspace convention says Rust projects use edition 2024. This is not a correctness issue and may be intentional for compatibility.
  Impact: minor consistency drift from the user's current Rust workspace standard.
  Fix: either document why 2021 is retained for this distributable MCP binary or schedule an edition-2024 migration once CI and Windows cross-build are green.

- Resolved Low - `scripts/fetch-binary.sh:45`
  The plugin installer treats a missing release checksum as best-effort and proceeds. That is pragmatic for older releases, but it is weaker than the current release workflow, which always publishes `.sha256`.
  Impact: a future release-regression or manually created release without checksum would silently reduce install integrity.
  Fix: after the next release that includes checksums, switch missing checksum from warning to hard failure, or gate best-effort behavior behind an explicit compatibility flag.

## Positive Notes

- Dependency hygiene improved narrowly: `sha2` and `shell-words` were added for concrete security/reliability fixes, and no broad dependency expansion was introduced.
- `Cargo.toml` keeps `rmcp` default features disabled and uses a trimmed Tokio feature set, which is appropriate for a stdio MCP server.
- CI already covers fmt, clippy with `-D warnings`, tests, and Windows MSVC cross-build smoke.
- Release workflow publishes SHA-256 sidecars, and the plugin fetch script verifies them when present.
- The codebase still follows the repo's layout standards: sibling test files, no `mod.rs`, and every inspected source file remains below 500 LOC.

## Verification

- Read `.full-review/00-scope.md`, `.full-review/01-quality-architecture.md`, `.full-review/02-security-performance.md`, and `.full-review/03-testing-documentation.md` before writing this phase.
- `wc -l src/*.rs src/bootstrap/*.rs README.md CLAUDE.md skills/ytdl/SKILL.md .claude-plugin/plugin.json .mcp.json gemini-extension.json .github/workflows/*.yml` - largest product source file is `src/service.rs` at 458 lines; no inspected source file exceeds 500 LOC.
- `rg -n "sha256|timeout|TransferTarget|RemoteSpec|RemotePath|run_command|status|partial|split|shell_words|aws-lc|xwin|cargo test|fmt|clippy|checksum|sha256sum" -S ...` - inspected best-practice relevant changes and docs references.
- `sed -n '195,230p' .../tokio-1.52.2/src/process/mod.rs`, `sed -n '1060,1095p' ...`, and `sed -n '1118,1130p' ...` - local Tokio docs/source confirm children continue by default after drop and `kill_on_drop` defaults to false.
- `cargo fmt --all --check` - passed.
- `jq empty .claude-plugin/plugin.json .mcp.json gemini-extension.json hooks/hooks.json` - passed.
