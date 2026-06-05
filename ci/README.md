# CI

`release.yml` is the GitHub Actions release workflow. It lives here (not under
`.github/workflows/`) because pushing workflow files requires a token with the
`workflow` OAuth scope.

To activate it:

```bash
gh auth refresh -s workflow        # one-time: add the workflow scope
mkdir -p .github/workflows
git mv ci/release.yml .github/workflows/release.yml
git commit -m "ci: activate release workflow" && git push
```

It builds `ytdl-mcp-linux-x86_64` and `ytdl-mcp-windows-x86_64.exe` on every
`v*` tag and attaches them to the GitHub Release; a separate job runs
`cargo test` + `cargo clippy -D warnings`.
