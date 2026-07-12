# MCP Apps North Star

This document is the reusable MCP Apps pattern for `ytdl-rmcp` and the other
rmcp-family servers. Use it when adding interactive UI resources, ChatGPT Apps
compatibility, Claude support, or Codex/plugin distribution.

The short rule: **standard MCP Apps first, host-specific extensions second**.
The portable contract is tools + resources + `text/html;profile=mcp-app` +
`_meta.ui.*`. OpenAI/ChatGPT, Claude, and Codex-specific behavior should layer
on top without becoming the core dependency.

## Current reference implementation

`youtube_search_ui` is the model implementation in this repo.

| Area | File |
| --- | --- |
| Tool/resource metadata | `src/search_app.rs` |
| Tool routing and app-callable tools | `src/mcp.rs` |
| Embedded widget | `assets/youtube-search-app.html` |
| Contract tests | `src/search_app_tests.rs`, `src/mcp_tests.rs` |
| User docs | `README.md`, `openwiki/architecture/overview.md` |

When copying the pattern to another server, copy the contract shape, not the
YouTube-specific UI.

## Build shape

An MCP app is still a normal MCP server. The UI layer is additive:

1. A tool returns useful text and/or structured data.
2. The tool descriptor advertises a UI resource with `_meta.ui.resourceUri`.
3. The server exposes that HTML through `resources/list` and `resources/read`.
4. A compatible host fetches the resource, renders it in an iframe, and sends
   the originating tool input/result to the iframe.
5. Hosts without app rendering still receive useful tool text/data.

Never return HTML from a tool call. Tool calls return data; resources return UI.

## Tool metadata

Every UI-backed tool should have:

```json
{
  "_meta": {
    "ui": {
      "resourceUri": "ui://server-name/widget.html"
    }
  }
}
```

For ChatGPT compatibility, also include:

```json
{
  "_meta": {
    "openai/outputTemplate": "ui://server-name/widget.html",
    "openai/toolInvocation/invoking": "Loading...",
    "openai/toolInvocation/invoked": "Ready"
  }
}
```

Keep status strings short. OpenAI documents a 64-character limit for invocation
status strings.

### App-callable tools

Any tool the iframe calls with `app.callServerTool(...)` should be explicitly
marked callable by both the model and the app:

```json
{
  "_meta": {
    "ui": {
      "visibility": ["model", "app"]
    },
    "openai/widgetAccessible": true,
    "openai/visibility": "public"
  }
}
```

Use `["app"]` only for helper tools that should never be selected directly by
the model. Use `["model", "app"]` for normal tools that are also invoked by the
widget. Avoid adding hidden helper tools unless the public tools are awkward or
unsafe for direct app calls.

### Output schemas

Declare `output_schema` / `outputSchema` for every tool that returns
`structuredContent`. This lets hosts validate results and lets models reason
about follow-up calls.

Preferred result shape:

```json
{
  "content": [{ "type": "text", "text": "Human-readable fallback." }],
  "structuredContent": {
    "items": []
  },
  "_meta": {
    "ui": {
      "resourceUri": "ui://server-name/widget.html"
    }
  }
}
```

The text fallback should be good enough for non-UI hosts.

## Resource metadata

Every app resource must use:

```text
text/html;profile=mcp-app
```

Do not use the old `text/html+skybridge` MIME type.

Every resource content should carry standard MCP Apps metadata:

```json
{
  "_meta": {
    "ui": {
      "csp": {
        "connectDomains": [],
        "resourceDomains": [],
        "frameDomains": [],
        "baseUriDomains": []
      },
      "permissions": {},
      "prefersBorder": true
    }
  }
}
```

Only include CSP fields you need. Empty/omitted domains mean block-by-default.
Run a CSP inventory before shipping: list every image, script, style, font,
fetch, WebSocket, iframe, and redirect origin the widget can touch.

For ChatGPT compatibility, mirror resource metadata:

```json
{
  "_meta": {
    "openai/widgetDescription": "Short description of what the widget shows.",
    "openai/widgetPrefersBorder": true,
    "openai/widgetCSP": {
      "connect_domains": [],
      "resource_domains": [],
      "frame_domains": [],
      "redirect_domains": []
    }
  }
}
```

`redirect_domains` is an OpenAI/ChatGPT extension for trusted
`window.openai.openExternal(...)` destinations. The standard MCP `ui.csp`
fields use camelCase; the OpenAI compatibility object uses snake_case.

Do not set `openai/widgetDomain` or `_meta.ui.domain` unless the app has a real
stable hosted origin. For local bundled resources, let the host sandbox choose
the default origin.

## Widget runtime

Use the MCP Apps bridge as the primary runtime:

```js
const { App } = window.McpExtApps || {};
const app = App ? new App({ name: "Widget", version: "1.0.0" }) : null;

if (app) app.ontoolinput = (params) => {};
if (app) app.ontoolresult = (result) => {};
if (app) app.onhostcontextchanged = (context) => {};

await app?.connect();
```

Register handlers before `connect()`. Hosts may send initial input/result events
as part of connection setup.

### Runtime adapters

Each widget should centralize host differences in small adapter functions:

| Intent | MCP Apps | ChatGPT fallback |
| --- | --- | --- |
| Call a server tool | `app.callServerTool({ name, arguments })` | `window.openai.callTool(name, args)` |
| Send a visible follow-up | `app.sendMessage({ role: "user", content })` | `window.openai.sendFollowUpMessage({ prompt })` |
| Open a URL | `app.openLink({ url })` | `window.openai.openExternal({ href })` |
| Request fullscreen/PIP | `app.requestDisplayMode({ mode })` | `window.openai.requestDisplayMode({ mode })` |
| Download generated data | `app.downloadFile({ contents })` | Browser download fallback if allowed |
| Update silent model context | `app.updateModelContext({ content, structuredContent })` | `window.openai.setWidgetState(...)` model content where useful |
| Log diagnostics | `app.sendLog(...)` | `console.*` fallback |
| Host context | `app.getHostContext()` / `onhostcontextchanged` | `window.openai.theme`, `locale`, `displayMode` |

Keep direct `window.openai` reads behind these helpers. The widget should run in
MCP-compatible hosts even when `window.openai` is absent.

### State

Use state for user continuity, not as a source of truth.

Recommended order:

1. Prefer host-provided input/result events for current data.
2. Use `window.openai.widgetState` / `setWidgetState` when ChatGPT provides it.
3. Use localStorage as a best-effort fallback.
4. Re-fetch through server tools when state is stale or missing.

For ChatGPT widget state, use a shape with:

```json
{
  "modelContent": [],
  "privateContent": {},
  "imageIds": []
}
```

Only put information in `modelContent` that the model should see in later
turns. Keep user-private or bulky UI state in `privateContent`.

## UX patterns

Pick one focused widget pattern per tool:

| Need | Recommended widget |
| --- | --- |
| Search large result sets | Search + results table/list |
| Inspect one item | Detail panel |
| Compare several items | Table or compact cards |
| Confirm destructive action | Confirmation panel; consider MCP elicitation first |
| Watch a long-running job | Progress/activity view |
| Show aggregate history | Dashboard/stat cards |

Use tabs or segmented controls when one tool naturally has multiple modes, such
as `Search | Stats`. Do not build a whole multi-page product inside one iframe
unless the workflow genuinely needs it.

Each widget should have:

- Loading state
- Empty state
- Error state with actionable language
- Non-UI text fallback from the tool
- Keyboard/focus support for primary controls
- Mobile-safe layout
- Host theme handling
- Safe-area awareness when available

## Security and safety

Treat the widget as untrusted UI running inside a strict sandbox:

- Escape all server/user data before injecting HTML.
- Route network work through server tools unless a direct browser request is
  necessary and declared in CSP.
- Use `app.openLink` / `window.openai.openExternal`; do not rely on
  `window.open` or raw links.
- Do not store secrets in widget state or localStorage.
- Declare tool annotations accurately: read-only, destructive, idempotent, and
  open-world behavior.
- Require confirmation for destructive writes. Use MCP elicitation when a simple
  yes/no or small enum is enough; use a widget when the user must inspect rich
  context first.
- Keep app-callable tools constrained by server-side validation. Metadata is a
  hint to the host, not authorization.

## Claude support

Claude/Anthropic support is primarily the MCP standard:

- Tools, resources, prompts, and server instructions should be correct without
  OpenAI-specific fields.
- Claude-capable app hosts consume `_meta.ui.resourceUri` and resource
  `_meta.ui.*` fields.
- `hostContext.safeAreaInsets` should be honored when present.
- Claude Code can reference MCP resources directly via `@server:uri` and
  provides list/read resource tools when a server supports resources.
- Claude Code tool search defers tool schemas until needed, so tool names and
  descriptions matter. Keep names direct and descriptions outcome-focused.
- Claude API MCP connector supports remote MCP servers and tool allow/deny
  configuration. Remote apps should prefer OAuth or no-auth read-only flows over
  static bearer tokens when preparing for broader listing.

Do not invent Claude-specific `_meta["claude/..."]` fields unless Anthropic
documents them. Keep Claude behavior on the standard MCP Apps surface.

## ChatGPT/OpenAI support

ChatGPT implements MCP Apps and continues to support OpenAI-specific Apps SDK
compatibility fields and `window.openai` helpers.

Always provide the standard MCP fields first:

- `_meta.ui.resourceUri`
- `_meta.ui.visibility`
- `_meta.ui.csp`
- `_meta.ui.prefersBorder`
- `_meta.ui.domain` only for hosted components with a real origin

Then add OpenAI compatibility fields where helpful:

- `_meta["openai/outputTemplate"]`
- `_meta["openai/widgetAccessible"]`
- `_meta["openai/visibility"]`
- `_meta["openai/toolInvocation/invoking"]`
- `_meta["openai/toolInvocation/invoked"]`
- `_meta["openai/widgetDescription"]`
- `_meta["openai/widgetPrefersBorder"]`
- `_meta["openai/widgetCSP"]`
- `_meta["openai/widgetDomain"]` only with a real hosted origin
- `_meta["openai/fileParams"]` for tools that accept ChatGPT file-library inputs

For apps intended for ChatGPT publishing, test behind HTTPS in developer mode
and verify direct, indirect, and negative prompts. Iterate on names,
descriptions, annotations, structured output, and `_meta` until the model calls
the right tool without over-prompting.

## Codex support

Codex support is not a separate iframe metadata namespace. It is a distribution,
workflow, and agent-ergonomics layer around MCP and plugins.

Codex-facing servers should provide:

- Clean MCP tools and resources over stdio and/or streamable HTTP.
- Useful server `instructions` during MCP initialization. Keep the first 512
  characters self-contained because Codex uses that guidance alongside tools.
- Project/user install paths for Codex MCP configuration.
- Plugin packaging when distributing reusable workflows. Plugins can include
  skills, apps/connectors, MCP servers, browser extensions, hooks, and scheduled
  templates.
- Skills that teach Codex how to use the server for common workflows.
- Hooks only when they are safe, inspectable, and worth the trust cost.
- Tool names and descriptions optimized for Codex tool search.
- Output schemas for structured results when the caller needs reliable
  downstream automation.

Codex-specific capabilities to account for:

- Shared MCP configuration across ChatGPT desktop app, Codex CLI, and IDE
  extension for the same Codex host.
- Sandbox and approval policies. Tools that touch local files, network, or
  shells should be explicit about effects and failure modes.
- Local stdio servers for project-scoped tools; streamable HTTP for shared or
  hosted systems.
- Plugins as the preferred package for reusable skills + MCP servers.
- `AGENTS.md` / `CLAUDE.md` instructions and repo docs as durable guidance for
  the agent.

Do not assume Codex currently renders every MCP Apps iframe the same way ChatGPT
or Claude does. Make the tool text/structured fallback useful so Codex remains a
good client even when the UI is ignored.

## Packaging and deployment

Keep packaging concerns separate from UI semantics:

| Package/deploy shape | Purpose | UI semantics |
| --- | --- | --- |
| npm launcher | Easy stdio install | Same MCP tools/resources |
| release binaries | Single-binary distribution | Same MCP tools/resources |
| `.mcpb` / `.dxt` | Claude Desktop/local bundle install | Same MCP tools/resources |
| plugin | ChatGPT/Codex distribution with skills/connectors/MCP | Same MCP tools/resources |
| hosted streamable HTTP | Remote app/server | Same MCP tools/resources |

`.mcpb` is not the UI mechanism. It packages a local server. The UI mechanism is
still the MCP Apps tool/resource contract.

## Testing checklist

Every MCP app change should have tests for:

- Tool descriptor advertises `_meta.ui.resourceUri`.
- Tool descriptor includes ChatGPT aliases when intended.
- App-callable tools advertise `_meta.ui.visibility` and
  `openai/widgetAccessible`.
- Resource MIME type is `text/html;profile=mcp-app`.
- Resource `_meta.ui.csp` includes every origin discovered in CSP inventory.
- Resource includes ChatGPT `openai/widgetCSP` aliases when intended.
- HTML contains the vendored MCP Apps bridge and no remote `esm.sh` dependency.
- HTML registers handlers before `connect()`.
- HTML includes host adapters for tool calls, messages, links, display mode,
  downloads, state, model context, and host context when those capabilities are
  part of the north-star pattern.
- Widget scripts parse in a plain JavaScript syntax check.
- Rust/host tests pass: `cargo test`, `cargo clippy --all-targets -- -D warnings`,
  `cargo fmt --all --check`, and packaging checks.
- A real host smoke test is run before release when UI behavior changes.

For `ytdl-rmcp`, the fast local checks are:

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
scripts/check-packaging.sh
node -e '/* parse embedded widget scripts */'
```

## Implementation checklist for new widgets

1. Decide if a widget is justified. Prefer plain text/structured output or MCP
   elicitation for simple forms and confirmations.
2. Define one user outcome for the widget.
3. Design the tool result payload first.
4. Add `outputSchema` for structured content.
5. Add standard `_meta.ui.*` metadata.
6. Add OpenAI compatibility aliases.
7. Register the HTML resource with `text/html;profile=mcp-app`.
8. Inventory CSP origins and encode both standard and OpenAI compatibility CSP.
9. Implement the widget with MCP Apps bridge primary and `window.openai`
   fallback.
10. Preserve text fallback for non-UI hosts.
11. Add contract tests before implementation changes.
12. Run the full verification checklist.
13. Document which pattern this widget is meant to teach future servers.

## Stats tab decision for this app

For `ytdl-rmcp`, `youtube_stats` is part of the existing app as a `Search |
Stats` segmented view.

Rationale:

- `youtube_stats` is read-only and already returns compact JSON.
- It demonstrates a dashboard/stat-card pattern reusable across homelab servers.
- It exercises app-callable tools beyond the search/probe/download flow.
- It does not introduce the local-file/path risks of `youtube_identify`.

`youtube_identify` should stay out of this widget for now. It is a separate
local-audio workflow with optional tag writes, so it deserves its own focused
widget with explicit confirmation before `write_tags=true`.

## Sources

- MCP Apps overview: https://modelcontextprotocol.io/extensions/apps/overview
- MCP Apps OpenAI migration guide:
  https://apps.extensions.modelcontextprotocol.io/api/documents/migrate-openai-app.html
- OpenAI Apps SDK reference: https://developers.openai.com/apps-sdk/reference
- OpenAI Codex MCP docs: https://developers.openai.com/codex/mcp
- OpenAI plugin docs: https://developers.openai.com/codex/plugins
- OpenAI ChatGPT app use case:
  https://developers.openai.com/codex/use-cases/chatgpt-apps
- Anthropic MCP overview: https://docs.anthropic.com/en/docs/agents-and-tools/mcp
- Anthropic Claude Code MCP docs: https://docs.anthropic.com/en/docs/claude-code/mcp
- Anthropic MCP connector docs:
  https://docs.anthropic.com/en/docs/agents-and-tools/mcp-connector
