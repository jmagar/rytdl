use rmcp::model::{ListResourcesResult, Meta, ReadResourceResult, Resource, ResourceContents};
use serde_json::json;

pub const RESOURCE_URI: &str = "ui://ytdl-rmcp/youtube-search.html";
pub const RESOURCE_MIME_TYPE: &str = "text/html;profile=mcp-app";
const HTML_TEMPLATE: &str = include_str!("../assets/youtube-search-app.html");
const APP_BRIDGE: &str = include_str!("../assets/ext-apps-vendored.js");
const APP_SCRIPT: &str = include_str!("../assets/youtube-search-app.js");
const APP_BRIDGE_PLACEHOLDER: &str = "{{MCP_EXT_APPS_BUNDLE}}";
const APP_SCRIPT_PLACEHOLDER: &str = "{{YOUTUBE_SEARCH_APP_SCRIPT}}";
const UI_META_KEY: &str = "ui";
const THUMBNAIL_DOMAINS: [&str; 2] = ["https://i.ytimg.com", "https://img.youtube.com"];
const EXTERNAL_LINK_DOMAINS: [&str; 4] = [
    "https://www.youtube.com",
    "https://youtu.be",
    "https://listen.plex.tv",
    "https://app.plex.tv",
];

pub fn list_app_resources() -> ListResourcesResult {
    ListResourcesResult {
        resources: vec![Resource::new(RESOURCE_URI, "youtube-search")
            .with_title("YouTube search")
            .with_description("Search YouTube and send results to ytdl-rmcp actions.")
            .with_mime_type(RESOURCE_MIME_TYPE)],
        next_cursor: None,
        meta: None,
    }
}

pub fn read_app_resource(uri: &str) -> Option<ReadResourceResult> {
    if uri != RESOURCE_URI {
        return None;
    }
    let meta = resource_meta();
    Some(ReadResourceResult::new(vec![ResourceContents::text(
        html(),
        RESOURCE_URI,
    )
    .with_mime_type(RESOURCE_MIME_TYPE)
    .with_meta(meta)]))
}

pub fn resource_meta() -> Meta {
    let mut meta = ui_meta(json!({
            "csp": {
                "connectDomains": [],
                "resourceDomains": [
                    "https://i.ytimg.com",
                    "https://img.youtube.com",
                    "https://listen.plex.tv",
                    "https://app.plex.tv"
                ],
                "baseUriDomains": []
            },
            "permissions": {
                "clipboardWrite": {}
            },
            "prefersBorder": true
    }));
    meta.0.insert(
        "openai/widgetDescription".into(),
        json!(
            "Interactive YouTube search results with probe, download, open-link, and follow-up actions."
        ),
    );
    meta.0
        .insert("openai/widgetPrefersBorder".into(), json!(true));
    meta.0.insert(
        "openai/widgetCSP".into(),
        json!({
            "connect_domains": [],
            "resource_domains": THUMBNAIL_DOMAINS,
            "redirect_domains": EXTERNAL_LINK_DOMAINS
        }),
    );
    meta
}

pub fn tool_meta() -> Meta {
    let mut meta = ui_meta(json!({ "resourceUri": RESOURCE_URI }));
    meta.0
        .insert("openai/outputTemplate".into(), json!(RESOURCE_URI));
    meta.0.insert(
        "openai/toolInvocation/invoking".into(),
        json!("Searching YouTube..."),
    );
    meta.0.insert(
        "openai/toolInvocation/invoked".into(),
        json!("Search ready"),
    );
    meta
}

pub fn app_callable_tool_meta() -> Meta {
    let mut meta = ui_meta(json!({ "visibility": ["model", "app"] }));
    meta.0.insert("openai/widgetAccessible".into(), json!(true));
    meta.0.insert("openai/visibility".into(), json!("public"));
    meta
}

fn ui_meta(value: serde_json::Value) -> Meta {
    let mut meta = Meta::new();
    meta.0.insert(UI_META_KEY.into(), value);
    meta
}

fn html() -> String {
    HTML_TEMPLATE
        .replace(APP_BRIDGE_PLACEHOLDER, APP_BRIDGE)
        .replace(APP_SCRIPT_PLACEHOLDER, APP_SCRIPT)
}

#[cfg(test)]
#[path = "search_app_tests.rs"]
mod tests;
