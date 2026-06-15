//! Build script: capture the short git commit SHA at compile time and expose it
//! as the `YTDL_GIT_SHA` env var for `env!`. Must never fail the build — when git
//! is absent or this is a packaged crate (no `.git`), it falls back to "unknown".

use std::process::Command;

fn main() {
    let sha = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=YTDL_GIT_SHA={sha}");

    // Best-effort: refresh the SHA when HEAD moves. Harmless if the path is absent.
    println!("cargo:rerun-if-changed=.git/HEAD");
}
