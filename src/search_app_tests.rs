use rmcp::model::ResourceContents;

#[test]
fn app_resource_uri_is_stable() {
    assert_eq!(super::RESOURCE_URI, "ui://ytdl-rmcp/youtube-search.html");
}

#[test]
fn app_resource_contains_html_and_aurora_hooks() {
    let result = super::read_app_resource(super::RESOURCE_URI).unwrap();
    let ResourceContents::TextResourceContents {
        text,
        mime_type,
        meta,
        ..
    } = &result.contents[0]
    else {
        panic!("expected text resource");
    };

    assert_eq!(mime_type.as_deref(), Some(super::RESOURCE_MIME_TYPE));
    assert!(text.contains("YouTube search"));
    assert!(text.contains("--aurora-page-bg"));
    assert!(text.contains("callServerTool"));
    assert!(text.contains("data-view=\"search\""));
    assert!(text.contains("data-view=\"stats\""));
    assert!(text.contains("loadStats"));
    assert!(text.contains("youtube_stats"));
    assert!(text.contains("renderStats"));
    assert!(text.contains("app.openLink"));
    assert!(text.contains("window.open"));
    assert!(text.contains("window.McpExtApps"));
    // The originating query arrives nested under `arguments` in the tool-input
    // notification; seeding the search box from a flat `params.query` leaves it
    // blank. Guard against regressing back to the shallow access.
    assert!(text.contains("params?.arguments?.query"));
    // Code-mode hosts reject widget callbacks to hidden tools; depending on the
    // transport that surfaces as a bare "tools/call failed: 404". friendlyError
    // must translate it into an actionable message instead of a raw 404.
    assert!(text.contains("tools/call failed:"));
    assert!(!text.contains("{{MCP_EXT_APPS_BUNDLE}}"));
    assert!(!text.contains("{{YOUTUBE_SEARCH_APP_SCRIPT}}"));
    assert!(!text.contains("https://esm.sh"));
    let ui = meta.as_ref().unwrap().0.get("ui").unwrap();
    assert_eq!(
        ui["csp"]["resourceDomains"][0],
        serde_json::json!("https://i.ytimg.com")
    );
    assert_eq!(ui["prefersBorder"], serde_json::json!(true));
    assert_eq!(
        meta.as_ref().unwrap().0["openai/widgetDescription"],
        serde_json::json!(
            "Interactive YouTube search results with probe, download, open-link, and follow-up actions."
        )
    );
    assert_eq!(
        meta.as_ref().unwrap().0["openai/widgetPrefersBorder"],
        serde_json::json!(true)
    );
    assert_eq!(
        meta.as_ref().unwrap().0["openai/widgetCSP"]["resource_domains"][0],
        serde_json::json!("https://i.ytimg.com")
    );
    assert_eq!(
        meta.as_ref().unwrap().0["openai/widgetCSP"]["redirect_domains"][0],
        serde_json::json!("https://www.youtube.com")
    );
}

#[test]
fn app_html_contains_playlist_and_transfers_views() {
    let html = super::html();
    assert!(html.contains("data-view=\"playlist\""));
    assert!(html.contains("data-view=\"transfers\""));
    assert!(!html.contains("{{YOUTUBE_SEARCH_APP_SCRIPT}}"));
}

#[test]
fn app_metadata_allows_plex_external_destinations() {
    let meta = super::resource_meta();
    let widget_csp = meta
        .0
        .get("openai/widgetCSP")
        .and_then(serde_json::Value::as_object)
        .unwrap();
    let redirects = widget_csp
        .get("redirect_domains")
        .and_then(serde_json::Value::as_array)
        .unwrap();
    assert!(redirects
        .iter()
        .any(|value| value == "https://listen.plex.tv"));
    assert!(redirects.iter().any(|value| value == "https://app.plex.tv"));
}

#[test]
fn tool_meta_links_to_app_resource() {
    let meta = super::tool_meta();

    assert_eq!(
        meta.0["ui"]["resourceUri"],
        serde_json::json!(super::RESOURCE_URI)
    );
    assert_eq!(
        meta.0["openai/outputTemplate"],
        serde_json::json!(super::RESOURCE_URI)
    );
    assert_eq!(
        meta.0["openai/toolInvocation/invoking"],
        serde_json::json!("Searching YouTube...")
    );
    assert_eq!(
        meta.0["openai/toolInvocation/invoked"],
        serde_json::json!("Search ready")
    );
}

#[test]
fn callable_tool_meta_marks_helpers_app_accessible() {
    let meta = super::app_callable_tool_meta();

    assert_eq!(
        meta.0["ui"]["visibility"],
        serde_json::json!(["model", "app"])
    );
    assert_eq!(meta.0["openai/widgetAccessible"], serde_json::json!(true));
    assert_eq!(meta.0["openai/visibility"], serde_json::json!("public"));
}
