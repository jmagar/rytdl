# Phase 2: Security and Performance

## Findings

- High - `src/transfer.rs:16`
  `ensure_remote_dir` passes `dest_path` as part of the remote SSH command: `ssh <remote> mkdir -p -- <dest_path>`. OpenSSH executes the remote command through the user's shell, so local argv boundaries are not a reliable quoting boundary for the remote command. Unlike the `scp` fallback, this path does not shell-quote `dest_path`.
  Impact: a destination containing spaces can be split incorrectly, and a malicious or malformed destination containing shell metacharacters can alter the remote command. Because `dest_path` can come from MCP input as well as environment configuration, this is a command-boundary injection risk.
  Fix: construct one quoted remote command string with a tested shell-quote function, for example `mkdir -p -- '<escaped dest>'`, or avoid the remote shell by using `sftp`/`scp` behavior that does not require a separate remote `mkdir`. Add tests for spaces, single quotes, semicolons, command substitutions, and leading dashes.

- High - `src/model.rs:128` and `src/transfer.rs:17`
  The `remote` value is accepted as an arbitrary tool input/config string and passed directly to `ssh`, `rsync`, and `scp`. There is no validation that it is an SSH alias or `user@host`, and no protection against values that begin with `-` or otherwise get interpreted as transport options by SSH-family tools.
  Impact: a tool caller can potentially influence local SSH behavior rather than only selecting a host. Even if typical MCP clients are trusted, this server exposes transfer controls through an automation interface and should defend the boundary.
  Fix: validate `remote` before use. Reject empty strings, whitespace, control characters, and leading `-`; consider allowing only SSH aliases and `user@host` forms with conservative characters. Apply the same validation before every transfer helper.

- Medium - `src/bootstrap/ytdlp.rs:44`, `src/bootstrap/ffmpeg.rs:26`, and `src/bootstrap/http.rs:11`
  Runtime bootstrap downloads executable code over HTTPS and installs it into the per-user cache without an application-level checksum, signature verification, or pinned version. The Claude plugin installer verifies the app release checksum when available, but the yt-dlp and ffmpeg bootstrap path does not add comparable verification.
  Impact: compromise or mis-release of the upstream asset path immediately becomes local code execution under the user's account. Auto-update means this can happen after the first install as well as on first run.
  Fix: support pinned versions and expected hashes for downloaded tools, verify upstream checksums/signatures where available, and document the trust model. At minimum, expose config to disable auto-update and pin known-good binaries for locked-down deployments.

- Medium - `src/service.rs:71` and `src/downloader.rs:141`
  Downloads are processed sequentially and each `yt-dlp` pass is collected with `Command::output()`, buffering stdout/stderr until the process exits. Large playlists can also print many `after_move` lines and error details.
  Impact: this is simple and usually fine for one-off downloads, but large batches or playlists can hold memory unnecessarily and keep later URLs waiting behind slow earlier URLs.
  Fix: stream stdout line-by-line, stream stderr into a bounded tail for errors, and consider bounded URL-level concurrency for independent URLs while keeping each URL's mode passes ordered.

- Medium - `src/downloader.rs:141`, `src/downloader.rs:239`, `src/transfer.rs:17`, `src/transfer.rs:56`, and `src/transfer.rs:89`
  External commands have no explicit timeout or cancellation policy. SSH is configured to avoid interactive prompts, but network stalls, remote hangs, or stuck media extraction can still pin a tool call indefinitely.
  Impact: one hung subprocess can tie up an MCP request and leave the caller with no bounded failure behavior. This is especially painful for stdio MCP clients because the tool call appears simply stuck.
  Fix: wrap subprocess waits in configurable `tokio::time::timeout`, use conservative defaults for transfer phases, and document that long media downloads can override the default timeout.

- Low - `.claude-plugin/plugin.json:57`
  The plugin's `archive_dir` description says the default is `~/.local/state/youtube_dl_mcp`, but the Rust code uses `ProjectDirs::from("tv", "tootie", "ytdl-mcp")`, which maps to a different application-specific state path.
  Impact: users may inspect or back up the wrong archive location.
  Fix: update the plugin description to match the actual default or expose the resolved default in setup output.

## Verification

- `cargo fmt --all --check` - passed.
- `cargo test --all` - passed; 15 tests passed.
- `cargo clippy --all-targets -- -D warnings` - passed.
- `cargo tree -i aws-lc-sys` - returned no matching package, confirming the documented `aws-lc-sys` cross-compile risk is not currently present.

## Critical Issues for Phase 3 Context

- Add tests around remote path quoting and remote validation before trusting the SSH transfer boundary.
- Add tests for partial success in `mode = both` so rendering and JSON output cannot contradict transfer behavior.
- Add docs for downloaded executable trust, version pinning, and timeout behavior.
