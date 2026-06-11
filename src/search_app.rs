use rmcp::model::{
    AnnotateAble, ListResourcesResult, Meta, RawResource, ReadResourceResult, ResourceContents,
};
use serde_json::json;

pub const RESOURCE_URI: &str = "ui://ytdl-mcp/youtube-search.html";
const HTML: &str = include_str!("../assets/youtube-search-app.html");

pub fn list_app_resources() -> ListResourcesResult {
    ListResourcesResult {
        resources: vec![RawResource::new(RESOURCE_URI, "youtube-search")
            .with_title("YouTube search")
            .with_description("Search YouTube and send results to ytdl-mcp actions.")
            .with_mime_type("text/html")
            .no_annotation()],
        next_cursor: None,
        meta: None,
    }
}

pub fn read_app_resource(uri: &str) -> Option<ReadResourceResult> {
    if uri != RESOURCE_URI {
        return None;
    }
    let mut meta = Meta::new();
    meta.0.insert(
        "ui.csp".into(),
        json!({
            "connect_domains": ["https://i.ytimg.com", "https://img.youtube.com"],
            "resource_domains": ["https://i.ytimg.com", "https://img.youtube.com"]
        }),
    );
    Some(ReadResourceResult::new(vec![ResourceContents::text(
        HTML,
        RESOURCE_URI,
    )
    .with_mime_type("text/html")
    .with_meta(meta)]))
}

pub fn tool_meta() -> Meta {
    let mut meta = Meta::new();
    meta.0.insert("ui.resourceUri".into(), json!(RESOURCE_URI));
    meta
}

#[cfg(test)]
#[path = "search_app_tests.rs"]
mod tests;
