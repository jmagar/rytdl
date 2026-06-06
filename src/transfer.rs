//! Transfer staged files to an SSH remote. Prefers `rsync`; falls back to `scp`
//! (which ships with OpenSSH on Linux and Windows, where rsync is absent).
//!
//! Key-based, non-interactive auth is assumed: `-o BatchMode=yes` plus
//! `-o StrictHostKeyChecking=accept-new` guarantee we fail fast rather than
//! hang on a prompt — a stdio MCP server has no TTY to answer one.

use std::path::Path;

use anyhow::{bail, Result};
use std::process::Output;

use tokio::process::Command;

use crate::util::command_error;

#[cfg(test)]
#[path = "transfer_tests.rs"]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteSpec(String);

impl RemoteSpec {
    pub fn parse(raw: impl Into<String>) -> Result<Self> {
        let raw = raw.into();
        if raw.trim().is_empty() {
            bail!("SSH remote must not be empty");
        }
        if raw.starts_with('-') {
            bail!("SSH remote must not start with '-'");
        }
        if raw.chars().any(|c| c.is_whitespace() || c.is_control()) {
            bail!("SSH remote must not contain whitespace or control characters");
        }
        Ok(Self(raw))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemotePath(String);

impl RemotePath {
    pub fn parse(raw: impl Into<String>) -> Result<Self> {
        let raw = raw.into();
        if raw.trim().is_empty() {
            bail!("remote destination path must not be empty");
        }
        if raw.chars().any(char::is_control) {
            bail!("remote destination path must not contain control characters");
        }
        Ok(Self(raw))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferTarget {
    remote: RemoteSpec,
    audio_dest: RemotePath,
    video_dest: RemotePath,
}

impl TransferTarget {
    pub fn parse(remote: &str, audio_dest: &str, video_dest: Option<&str>) -> Result<Self> {
        let remote = RemoteSpec::parse(remote)?;
        let audio_dest = RemotePath::parse(audio_dest)?;
        let video_dest = match video_dest {
            Some(path) => RemotePath::parse(path)?,
            None => audio_dest.clone(),
        };
        Ok(Self {
            remote,
            audio_dest,
            video_dest,
        })
    }

    pub fn remote(&self) -> &RemoteSpec {
        &self.remote
    }

    pub fn audio_dest(&self) -> &RemotePath {
        &self.audio_dest
    }

    pub fn video_dest(&self) -> &RemotePath {
        &self.video_dest
    }
}

/// Ensure the destination directory tree exists on the remote (idempotent).
pub async fn ensure_remote_dir(
    remote: &RemoteSpec,
    dest_path: &RemotePath,
    ssh_opts: &[String],
) -> Result<()> {
    let command = remote_mkdir_command(dest_path);
    let mut cmd = Command::new("ssh");
    cmd.args(ssh_opts).arg(remote.as_str()).arg(&command);
    let out = command_output(&mut cmd).await?;
    if !out.status.success() {
        let detail = command_error(&out);
        let remote = remote.as_str();
        let dest_path = dest_path.as_str();
        bail!(
            "could not create '{dest_path}' on '{remote}': {detail}. \
             Check the remote alias, your SSH key, and write permissions."
        );
    }
    Ok(())
}

/// Sync the *contents* of `staging_kind_dir` (an `audio/` or `video/` subdir,
/// including its artist folders) into `dest_path` on `remote`.
pub async fn transfer(
    staging_kind_dir: &Path,
    remote: &RemoteSpec,
    dest_path: &RemotePath,
    ssh_opts: &[String],
) -> Result<()> {
    if which::which("rsync").is_ok() {
        rsync(staging_kind_dir, remote, dest_path, ssh_opts).await
    } else {
        scp(staging_kind_dir, remote, dest_path, ssh_opts).await
    }
}

async fn rsync(
    dir: &Path,
    remote: &RemoteSpec,
    dest_path: &RemotePath,
    ssh_opts: &[String],
) -> Result<()> {
    // Trailing slash on the source copies the contents, not the dir itself.
    let src = format!("{}/", dir.display());
    // `-s`/`--protect-args` sends the path to the remote rsync directly,
    // bypassing remote-shell word-splitting — so spaces are safe and the path
    // must NOT be shell-quoted (quoting would make it parse as relative).
    let target = format!("{}:{}/", remote.as_str(), dest_path.as_str());
    let ssh_cmd = rsync_remote_shell_command(ssh_opts);
    let mut cmd = Command::new("rsync");
    cmd.args(["-av", "--partial", "--human-readable", "-s", "-e", &ssh_cmd])
        .arg(&src)
        .arg(&target);
    let out = command_output(&mut cmd).await?;
    if !out.status.success() {
        bail!(
            "rsync failed (exit {:?}): {}",
            out.status.code(),
            command_error(&out)
        );
    }
    Ok(())
}

async fn scp(
    dir: &Path,
    remote: &RemoteSpec,
    dest_path: &RemotePath,
    ssh_opts: &[String],
) -> Result<()> {
    // scp has no "contents of dir" mode like rsync's trailing slash, so copy
    // each top-level entry (artist folders) recursively into the dest.
    let mut entries = Vec::new();
    for e in std::fs::read_dir(dir)? {
        entries.push(e?.path());
    }
    if entries.is_empty() {
        return Ok(());
    }
    let target = format!("{}:{}/", remote.as_str(), shell_quote(dest_path.as_str()));
    let mut cmd = Command::new("scp");
    cmd.arg("-r").args(ssh_opts);
    for e in &entries {
        cmd.arg(e);
    }
    cmd.arg(&target);
    let out = command_output(&mut cmd).await?;
    if !out.status.success() {
        bail!(
            "scp failed (exit {:?}): {}",
            out.status.code(),
            command_error(&out)
        );
    }
    Ok(())
}

/// Minimal single-quote shell escaping for the remote path (survives spaces).
fn remote_mkdir_command(dest_path: &RemotePath) -> String {
    format!("mkdir -p -- {}", shell_quote(dest_path.as_str()))
}

fn rsync_remote_shell_command(ssh_opts: &[String]) -> String {
    std::iter::once("ssh".to_string())
        .chain(ssh_opts.iter().map(|arg| shell_quote_if_needed(arg)))
        .collect::<Vec<_>>()
        .join(" ")
}

async fn command_output(cmd: &mut Command) -> Result<Output> {
    cmd.kill_on_drop(true);
    Ok(cmd.output().await?)
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

fn shell_quote_if_needed(s: &str) -> String {
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "@%_+=:,./-".contains(c))
    {
        s.to_string()
    } else {
        shell_quote(s)
    }
}
