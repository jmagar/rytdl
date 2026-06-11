use rmcp::model::ResourceContents;

#[test]
fn app_resource_uri_is_stable() {
    assert_eq!(super::RESOURCE_URI, "ui://ytdl-mcp/youtube-search.html");
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

    assert_eq!(mime_type.as_deref(), Some("text/html"));
    assert!(text.contains("YouTube search"));
    assert!(text.contains("--aurora-page-bg"));
    assert!(meta.as_ref().unwrap().0.contains_key("ui.csp"));
}
