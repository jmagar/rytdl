use super::*;

#[test]
fn remote_mkdir_command_quotes_shell_sensitive_paths() {
    // All inputs are valid absolute paths (a leading-dash path is rejected by
    // RemotePath::parse and is covered separately below); the point here is
    // that shell-sensitive characters in an accepted path are quoted safely.
    let cases = [
        ("/media/music library", "mkdir -p -- '/media/music library'"),
        ("/media/O'Brien", "mkdir -p -- '/media/O'\\''Brien'"),
        (
            "/media/a; touch pwned",
            "mkdir -p -- '/media/a; touch pwned'",
        ),
        (
            "/media/$(touch pwned)",
            "mkdir -p -- '/media/$(touch pwned)'",
        ),
        ("/-dash/child", "mkdir -p -- '/-dash/child'"),
    ];

    for (raw, expected) in cases {
        let path = RemotePath::parse(raw).unwrap();
        assert_eq!(remote_mkdir_command(&path), expected);
    }
}

#[test]
fn remote_path_accepts_normal_absolute_path() {
    assert_eq!(
        RemotePath::parse("/srv/music").unwrap().as_str(),
        "/srv/music"
    );
    // Embedded spaces are fine; they are shell-quoted downstream.
    assert_eq!(
        RemotePath::parse("/media/music library").unwrap().as_str(),
        "/media/music library"
    );
}

#[test]
fn remote_path_rejects_unsafe_values_with_typed_errors() {
    use TransferValidationError as E;
    let field = "remote destination path";

    let cases: [(&str, E); 7] = [
        ("", E::Empty { field }),
        ("   ", E::Empty { field }),
        ("/media/\u{7}bell", E::BadChars { field }),
        ("-oProxyCommand=sh", E::LeadingDash { field }),
        ("relative/path", E::NotAbsolute { field }),
        ("music", E::NotAbsolute { field }),
        ("/music/../../etc", E::Traversal { field }),
    ];

    for (raw, expected) in cases {
        let err = RemotePath::parse_typed(raw).expect_err(&format!("{raw:?} should be rejected"));
        assert_eq!(err, expected, "wrong variant for {raw:?}");
    }
}

#[test]
fn remote_path_rejects_traversal_and_relative_via_public_parse() {
    // Public anyhow-returning surface still rejects these.
    assert!(RemotePath::parse("/music/../etc").is_err());
    assert!(RemotePath::parse("..").is_err());
    assert!(RemotePath::parse("~/.config/autostart").is_err());
    assert!(RemotePath::parse("-leading").is_err());
}

#[test]
fn remote_rejects_option_like_empty_whitespace_and_control_values() {
    for raw in [
        "",
        "   ",
        "-oProxyCommand=sh",
        "host name",
        "host\tname",
        "host\nname",
    ] {
        assert!(
            RemoteSpec::parse(raw).is_err(),
            "{raw:?} should be rejected"
        );
    }
}

#[test]
fn remote_accepts_common_ssh_aliases_and_user_hosts() {
    for raw in [
        "nas",
        "music-box",
        "user@example.com",
        "user.name@host.local:2222",
    ] {
        assert_eq!(RemoteSpec::parse(raw).unwrap().as_str(), raw);
    }
}

#[test]
fn transfer_target_validates_all_boundaries_once() {
    let target = TransferTarget::parse("nas", "/audio library", Some("/videos")).unwrap();
    assert_eq!(target.remote().as_str(), "nas");
    assert_eq!(target.audio_dest().as_str(), "/audio library");
    assert_eq!(target.video_dest().as_str(), "/videos");

    assert!(TransferTarget::parse("-bad", "/audio", None).is_err());
    assert!(TransferTarget::parse("nas", "   ", None).is_err());
    assert!(TransferTarget::parse("nas", "\n/audio", None).is_err());
}

#[test]
fn rsync_remote_shell_command_quotes_each_ssh_arg() {
    let opts = vec![
        "-o".to_string(),
        "BatchMode=yes".to_string(),
        "-i".to_string(),
        "/home/me/keys/media key".to_string(),
        "-o".to_string(),
        "ProxyCommand=ssh jump 'nc %h %p'".to_string(),
    ];

    assert_eq!(
        rsync_remote_shell_command(&opts),
        "ssh -o BatchMode=yes -i '/home/me/keys/media key' -o 'ProxyCommand=ssh jump '\\''nc %h %p'\\'''"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn dropped_transfer_command_kills_child_process() {
    use std::process::Command as StdCommand;
    use std::time::Duration;

    let dir = tempfile::tempdir().unwrap();
    let pid_path = dir.path().join("child.pid");
    let script = format!("printf $$ > {}; exec sleep 5", pid_path.display());

    let mut cmd = tokio::process::Command::new("sh");
    cmd.args(["-c", &script]);

    let result = tokio::time::timeout(
        Duration::from_millis(100),
        run_capped(&mut cmd, None, Some(STDERR_CAP)),
    )
    .await;
    assert!(result.is_err(), "command should still be sleeping");

    let pid = std::fs::read_to_string(&pid_path)
        .unwrap()
        .trim()
        .to_string();
    tokio::time::sleep(Duration::from_millis(100)).await;

    let alive = StdCommand::new("kill")
        .args(["-0", &pid])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(
        !alive.success(),
        "timed-out transfer command left child process {pid} alive"
    );
}
