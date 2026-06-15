use super::*;

fn sample_envs() -> Vec<(String, String)> {
    vec![
        ("YTDLP_REMOTE".to_string(), "nas".to_string()),
        ("YTDLP_REMOTE_PATH".to_string(), "/music".to_string()),
    ]
}

#[test]
fn claude_places_env_flags_before_separator_and_cmd_after() {
    let envs = sample_envs();
    let args = build_mcp_add_args("claude", "ytdl-mcp", "/usr/bin/ytdl-mcp", &envs);
    assert_eq!(
        args,
        vec![
            "mcp",
            "add",
            "-s",
            "user",
            "ytdl-mcp",
            "-e",
            "YTDLP_REMOTE=nas",
            "-e",
            "YTDLP_REMOTE_PATH=/music",
            "--",
            "/usr/bin/ytdl-mcp",
        ]
    );

    // The `--` separator must come after every `-e` flag and immediately before
    // the command, so env values are never parsed as the trailing command.
    let sep = args.iter().position(|a| a == "--").unwrap();
    let last_e = args.iter().rposition(|a| a == "-e").unwrap();
    assert!(
        last_e < sep,
        "claude: -e flags must precede the -- separator"
    );
    assert_eq!(args.last().unwrap(), "/usr/bin/ytdl-mcp");
    assert_eq!(args[sep + 1], "/usr/bin/ytdl-mcp");
}

#[test]
fn codex_uses_env_flag_before_name() {
    let envs = sample_envs();
    let args = build_mcp_add_args("codex", "ytdl-mcp", "/usr/bin/ytdl-mcp", &envs);
    assert_eq!(
        args,
        vec![
            "mcp",
            "add",
            "--env",
            "YTDLP_REMOTE=nas",
            "--env",
            "YTDLP_REMOTE_PATH=/music",
            "ytdl-mcp",
            "--",
            "/usr/bin/ytdl-mcp",
        ]
    );

    // codex uses `--env` (not `-e`) and every `--env` flag must come BEFORE the
    // server name positional.
    assert!(
        !args.iter().any(|a| a == "-e"),
        "codex must use --env, not -e"
    );
    let name = args.iter().position(|a| a == "ytdl-mcp").unwrap();
    let last_env = args.iter().rposition(|a| a == "--env").unwrap();
    assert!(last_env < name, "codex: --env flags must precede the name");
}

#[test]
fn gemini_places_env_flags_after_name_and_cmd() {
    let envs = sample_envs();
    let args = build_mcp_add_args("gemini", "ytdl-mcp", "/usr/bin/ytdl-mcp", &envs);
    assert_eq!(
        args,
        vec![
            "mcp",
            "add",
            "-s",
            "user",
            "ytdl-mcp",
            "/usr/bin/ytdl-mcp",
            "-e",
            "YTDLP_REMOTE=nas",
            "-e",
            "YTDLP_REMOTE_PATH=/music",
        ]
    );

    // gemini puts the env (`-e`) flags LAST — after both the name and command
    // positionals, and there is no `--` separator.
    assert!(
        !args.iter().any(|a| a == "--"),
        "gemini uses no -- separator"
    );
    let name = args.iter().position(|a| a == "ytdl-mcp").unwrap();
    let cmd = args.iter().position(|a| a == "/usr/bin/ytdl-mcp").unwrap();
    let first_e = args.iter().position(|a| a == "-e").unwrap();
    assert!(name < cmd, "gemini: name must precede command");
    assert!(
        cmd < first_e,
        "gemini: env flags must come after the command"
    );
}

#[test]
fn unknown_bin_falls_back_to_claude_shape() {
    let envs = sample_envs();
    let claude = build_mcp_add_args("claude", "ytdl-mcp", "/bin/x", &envs);
    let other = build_mcp_add_args("something-else", "ytdl-mcp", "/bin/x", &envs);
    assert_eq!(claude, other);
}

#[test]
fn no_envs_produces_minimal_argv_per_cli() {
    let envs: Vec<(String, String)> = vec![];
    assert_eq!(
        build_mcp_add_args("claude", "ytdl-mcp", "/bin/x", &envs),
        vec!["mcp", "add", "-s", "user", "ytdl-mcp", "--", "/bin/x"]
    );
    assert_eq!(
        build_mcp_add_args("codex", "ytdl-mcp", "/bin/x", &envs),
        vec!["mcp", "add", "ytdl-mcp", "--", "/bin/x"]
    );
    assert_eq!(
        build_mcp_add_args("gemini", "ytdl-mcp", "/bin/x", &envs),
        vec!["mcp", "add", "-s", "user", "ytdl-mcp", "/bin/x"]
    );
}
