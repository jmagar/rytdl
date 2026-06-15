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

use crate::util::{command_error, run_output};

#[cfg(test)]
#[path = "transfer_tests.rs"]
mod tests;

/// Typed failures for the transfer input-validation boundary (`RemoteSpec` /
/// `RemotePath`). Hand-rolled `Error`/`Display` (the crate does not depend on
/// `thiserror`) so tests can assert on a specific variant rather than
/// string-matching, while `parse` still surfaces as `anyhow::Result` to callers
/// (anyhow auto-converts any `std::error::Error`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferValidationError {
    /// Value was empty or only whitespace.
    Empty { field: &'static str },
    /// Value started with `-`, so a shell/command could read it as an option.
    LeadingDash { field: &'static str },
    /// Value contained whitespace and/or control characters.
    BadChars { field: &'static str },
    /// Path contained a `..` segment (directory traversal).
    Traversal { field: &'static str },
    /// Path was not absolute (did not start with `/`).
    NotAbsolute { field: &'static str },
}

impl std::fmt::Display for TransferValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty { field } => write!(f, "{field} must not be empty"),
            Self::LeadingDash { field } => write!(f, "{field} must not start with '-'"),
            Self::BadChars { field } => {
                // Shared variant: `RemotePath` rejects only control characters
                // (whitespace is allowed in names like "Title [id]"), while
                // `RemoteSpec` additionally rejects whitespace. Keep the message
                // truthful for both uses rather than over-claiming for the path.
                write!(
                    f,
                    "{field} must not contain control characters (the SSH remote also rejects whitespace)"
                )
            }
            Self::Traversal { field } => {
                write!(f, "{field} must not contain a '..' path segment")
            }
            Self::NotAbsolute { field } => {
                write!(f, "{field} must be an absolute path starting with '/'")
            }
        }
    }
}

impl std::error::Error for TransferValidationError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteSpec(String);

impl RemoteSpec {
    pub fn parse(raw: impl Into<String>) -> Result<Self> {
        Ok(Self::parse_typed(raw)?)
    }

    fn parse_typed(raw: impl Into<String>) -> std::result::Result<Self, TransferValidationError> {
        const FIELD: &str = "SSH remote";
        let raw = raw.into();
        if raw.trim().is_empty() {
            return Err(TransferValidationError::Empty { field: FIELD });
        }
        if raw.starts_with('-') {
            return Err(TransferValidationError::LeadingDash { field: FIELD });
        }
        if raw.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(TransferValidationError::BadChars { field: FIELD });
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
        Ok(Self::parse_typed(raw)?)
    }

    /// Validate a remote destination path. The remote layout is `Artist/Title
    /// [id]` under an absolute media root (documented as "Absolute remote dir"
    /// for both `YTDLP_REMOTE_PATH` and the `dest_path` tool input), so we
    /// require an absolute path and reject anything that could redirect writes
    /// outside that root or be read as a command-line option:
    ///   - empty / whitespace-only
    ///   - any control character
    ///   - a leading `-` (option-injection defense, matching `RemoteSpec`)
    ///   - non-absolute paths (must start with `/`)
    ///   - any `..` path segment (directory traversal)
    fn parse_typed(raw: impl Into<String>) -> std::result::Result<Self, TransferValidationError> {
        const FIELD: &str = "remote destination path";
        let raw = raw.into();
        if raw.trim().is_empty() {
            return Err(TransferValidationError::Empty { field: FIELD });
        }
        if raw.chars().any(char::is_control) {
            return Err(TransferValidationError::BadChars { field: FIELD });
        }
        if raw.starts_with('-') {
            return Err(TransferValidationError::LeadingDash { field: FIELD });
        }
        if !raw.starts_with('/') {
            return Err(TransferValidationError::NotAbsolute { field: FIELD });
        }
        if raw.split('/').any(|segment| segment == "..") {
            return Err(TransferValidationError::Traversal { field: FIELD });
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

/// Run a transfer subprocess (ssh/rsync/scp) through the shared runner. The
/// caller in `service.rs` applies its own external timeout around this future,
/// so no internal timeout is passed (`None`); the shared runner still gives us
/// `ETXTBSY` retry, process-group placement, and `kill_on_drop`.
async fn command_output(cmd: &mut Command) -> Result<Output> {
    run_output(cmd).await
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
