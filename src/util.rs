//! Small shared helpers, including the single subprocess runner used by the
//! downloader, probe, fingerprinter, and transfer paths.

use std::process::{ExitStatus, Output, Stdio};
use std::time::Duration;

use anyhow::{bail, Result};
use serde_json::Value;
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};

/// `ETXTBSY` ("text file busy"): a Unix-only race that fires when we try to
/// exec a binary we *just* downloaded/wrote and the file is still held open for
/// writing elsewhere. We retry the spawn a few times to let the writer close.
const ETXTBSY: i32 = 26;

/// How many times to retry a spawn that fails with `ETXTBSY`.
const SPAWN_RETRIES: u32 = 5;

/// Best-effort error text from a failed subprocess: trimmed stderr, falling
/// back to stdout when stderr is empty.
pub fn command_error(out: &Output) -> String {
    let err = String::from_utf8_lossy(&out.stderr);
    let err = err.trim();
    if err.is_empty() {
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    } else {
        err.to_string()
    }
}

/// Non-empty trimmed string field from a JSON value, or `None`.
pub fn json_str(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
}

/// Captured result of a [`run_capped`] invocation.
#[derive(Debug)]
pub struct CommandOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    /// stderr, possibly tail-truncated per the `stderr_tail_cap` argument.
    pub stderr: String,
}

/// Spawn `cmd`, capture stdout (in full) and stderr (in full or tail-capped),
/// and enforce an optional timeout. This is the single subprocess runner shared
/// by yt-dlp, fpcalc, ssh/rsync/scp call sites.
///
/// - On Unix the child is placed in its own process group (`process_group(0)`)
///   before spawn so a future group-kill can reap grandchildren (yt-dlp→ffmpeg,
///   rsync→ssh). `kill_on_drop(true)` is always set as the portable safety net.
/// - Spawn is retried on `ETXTBSY`.
/// - `stderr_tail_cap = Some(n)` keeps only the last `n` bytes of stderr (with a
///   truncation marker); `None` captures stderr in full.
/// - `timeout = None` means no internal timeout (callers that wrap the future in
///   an external timeout pass `None`; `kill_on_drop` cleans up on drop).
pub async fn run_capped(
    cmd: &mut Command,
    timeout: Option<Duration>,
    stderr_tail_cap: Option<usize>,
) -> Result<CommandOutput> {
    cmd.stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    {
        // `process_group` is an inherent method on tokio's Command. Owning a
        // process group lets a group-targeted kill reach grandchildren.
        cmd.process_group(0);
    }
    let mut child = spawn_with_retry(cmd).await?;
    let mut stdout = child.stdout.take().expect("stdout piped");
    let mut stderr = child.stderr.take().expect("stderr piped");

    let stdout_task = tokio::spawn(async move {
        let mut out = Vec::new();
        stdout.read_to_end(&mut out).await.map(|_| out)
    });
    let stderr_task = tokio::spawn(async move {
        let mut buf = [0_u8; 8192];
        let mut tail = Vec::new();
        loop {
            let read = stderr.read(&mut buf).await?;
            if read == 0 {
                break;
            }
            match stderr_tail_cap {
                Some(limit) => append_tail(&mut tail, &buf[..read], limit),
                None => tail.extend_from_slice(&buf[..read]),
            }
        }
        let text = match stderr_tail_cap {
            Some(limit) => stderr_tail_text(&tail, limit),
            None => String::from_utf8_lossy(&tail).to_string(),
        };
        Ok::<_, std::io::Error>(text)
    });

    let status = if let Some(limit) = timeout {
        match tokio::time::timeout(limit, child.wait()).await {
            Ok(status) => status?,
            Err(_) => {
                kill_process_group(&mut child).await;
                bail!("command timed out after {}s", limit.as_secs());
            }
        }
    } else {
        child.wait().await?
    };

    let stdout = stdout_task.await??;
    let stderr = stderr_task.await??;
    Ok(CommandOutput {
        status,
        stdout,
        stderr,
    })
}

/// Convenience over [`run_capped`] that materializes a plain
/// [`std::process::Output`] (full stderr). Used by call sites whose callers
/// apply their own external timeout and expect an `Output`.
pub async fn run_output(cmd: &mut Command) -> Result<Output> {
    let captured = run_capped(cmd, None, None).await?;
    Ok(Output {
        status: captured.status,
        stdout: captured.stdout,
        stderr: captured.stderr.into_bytes(),
    })
}

/// Kill the child. On Unix, signal the whole process group so grandchildren
/// (ffmpeg under yt-dlp, ssh under rsync) are reaped, not just the direct child.
async fn kill_process_group(child: &mut Child) {
    #[cfg(unix)]
    {
        // The child was spawned with `process_group(0)`, so its PID is also its
        // PGID. Negating the PID targets the whole group with SIGKILL.
        if let Some(pid) = child.id() {
            // SAFETY: `kill(2)` with a negative pid signals the whole process
            // group. We declare the symbol directly rather than depending on the
            // `libc` crate (Cargo.toml is unchanged) — `kill` is part of the C
            // runtime that every Unix Rust binary already links.
            const SIGKILL: i32 = 9;
            let killed = unsafe { libc_kill(-(pid as i32), SIGKILL) };
            if killed == 0 {
                // Reap so we don't leave a zombie; ignore errors.
                let _ = child.wait().await;
                return;
            }
        }
    }
    // Non-Unix, or pid unavailable / group-kill failed: best-effort direct kill.
    let _ = child.kill().await;
}

#[cfg(unix)]
extern "C" {
    /// `kill(2)` from the C library. A negative `pid` targets a process group.
    #[link_name = "kill"]
    fn libc_kill(pid: i32, sig: i32) -> i32;
}

async fn spawn_with_retry(cmd: &mut Command) -> std::io::Result<Child> {
    let mut attempts = 0;
    loop {
        match cmd.spawn() {
            Ok(child) => return Ok(child),
            Err(error) if is_executable_busy(&error) && attempts < SPAWN_RETRIES => {
                attempts += 1;
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            Err(error) => return Err(error),
        }
    }
}

fn is_executable_busy(error: &std::io::Error) -> bool {
    error.raw_os_error() == Some(ETXTBSY)
}

fn append_tail(tail: &mut Vec<u8>, chunk: &[u8], limit: usize) {
    tail.extend_from_slice(chunk);
    if tail.len() > limit {
        let excess = tail.len() - limit;
        tail.drain(..excess);
    }
}

/// Render a captured (tail-capped) stderr buffer to text, prefixing a marker and
/// dropping the leading partial line when truncation occurred.
pub(crate) fn stderr_tail_text(bytes: &[u8], limit: usize) -> String {
    let truncated = bytes.len() >= limit;
    let mut text = String::from_utf8_lossy(bytes).to_string();
    if truncated {
        if let Some(pos) = text.find('\n') {
            text.drain(..=pos);
        }
        text.insert_str(0, "[stderr truncated]\n");
    }
    text
}
