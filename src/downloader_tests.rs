use std::time::Duration;

use tokio::process::Command;

use super::*;

#[test]
fn stderr_tail_keeps_last_complete_lines_with_truncation_marker() {
    let input = b"one\ntwo\nthree\nfour\n";
    let tail = stderr_tail_text(input, 10);

    assert!(tail.starts_with("[stderr truncated]\n"));
    assert!(tail.contains("three\nfour"));
    assert!(!tail.contains("one\n"));
}

#[tokio::test]
async fn run_command_reports_timeout() {
    let mut cmd = Command::new("sh");
    cmd.args(["-c", "sleep 2"]);

    let err = run_command(&mut cmd, Some(Duration::from_millis(50)))
        .await
        .unwrap_err()
        .to_string();

    assert!(err.contains("timed out after"));
}
