# CI

These are the GitHub Actions workflows. They live here, not under
`.github/workflows/`, because pushing workflow files requires a token with the
`workflow` OAuth scope.

- `ci.yml` — fmt + clippy + tests + a Windows cross-build, on every push/PR.
- `release.yml` — builds `ytdl-mcp-linux-x86_64` and `ytdl-mcp-windows-x86_64.exe`
  on every `v*` tag and attaches them to the GitHub Release.

To activate them:

```bash
gh auth refresh -s workflow -h github.com   # one-time: complete the device flow
mkdir -p .github/workflows
git mv ci/ci.yml ci/release.yml .github/workflows/
git rm ci/README.md
git commit -m "ci: activate workflows" && git push
```
