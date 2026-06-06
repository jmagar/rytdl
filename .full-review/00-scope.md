# Review Scope

## Target

Entire `/home/jmagar/workspace/ytdl-mcp` repository on `main`.

## Files

- `Cargo.toml`
- `Cargo.lock`
- `README.md`
- `CLAUDE.md`
- `AGENTS.md`
- `GEMINI.md`
- `src/`
- `.github/workflows/`
- `.claude-plugin/`
- `scripts/`
- `hooks/`
- `gemini-extension.json`
- `skills/ytdl/SKILL.md`

## Review Flags

- Security focus: yes
- Performance critical: yes
- Strict mode: yes
- Framework: Rust MCP server using `rmcp`, `tokio`, `yt-dlp`, `ffmpeg`, `ssh`, `rsync`, and `scp`

## Review Phases

1. Code Quality and Architecture
2. Security and Performance
3. Testing and Documentation
4. Best Practices and Standards
5. Consolidated Report

## Baseline Commands

- `git status --short --branch` - passed; worktree clean on `main...origin/main`.
- `cargo fmt --all --check` - passed.
- `cargo test --all` - passed; 15 tests passed.
- `cargo clippy --all-targets -- -D warnings` - passed.
- `cargo tree -i aws-lc-sys` - returned no matching package, confirming `aws-lc-sys` is absent.
