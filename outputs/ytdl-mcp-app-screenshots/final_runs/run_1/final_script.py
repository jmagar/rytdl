from pathlib import Path
from playwright.sync_api import sync_playwright

ROOT = Path('/home/jmagar/workspace/ytdl-mcp')
RUN = ROOT / 'outputs/ytdl-mcp-app-screenshots/final_runs/run_1'
SHOTS = RUN / 'screenshots'
LOG = RUN / 'final_script_log.txt'

def expanded_html() -> str:
    html = (ROOT / 'assets/youtube-search-app.html').read_text()
    script = (ROOT / 'assets/youtube-search-app.js').read_text()
    shim = r'''
window.McpExtApps = {
  App: class {
    constructor() {}
    getHostContext() { return { theme: 'dark', displayMode: 'inline', locale: 'en-US', availableDisplayModes: ['inline', 'fullscreen'] }; }
    connect() { return Promise.resolve({}); }
    sendLog() {}
    updateModelContext() { return Promise.resolve({}); }
    sendMessage() { return Promise.resolve({}); }
    openLink() { return Promise.resolve({}); }
    requestDisplayMode() { return Promise.resolve({}); }
    downloadFile() { return Promise.resolve({}); }
    callServerTool({ name }) {
      if (name === 'youtube_stats') return Promise.resolve({ structuredContent: {
        total_downloads: 42, total_files: 67, size: '5.8 GB', total_bytes: 6227702579, skipped_entries: 1,
        by_kind: { audio: { files: 51 }, video: { files: 16 } },
        recent: [
          { title: 'Slow Pulp - Falling Apart Live', status: 'ok', timestamp: '2026-07-12 09:14' },
          { title: 'Japanese Breakfast - Live Session', status: 'ok', timestamp: '2026-07-12 08:50' },
          { title: 'Turnstile - Tiny Desk', status: 'partial', timestamp: '2026-07-11 22:03' }
        ]
      }});
      return Promise.resolve({ structuredContent: {
        query: 'slow pulp live', limit: 10,
        results: [
          { title: 'Slow Pulp - Falling Apart Live', url: 'https://www.youtube.com/watch?v=abc123', uploader: 'Slow Pulp', duration: 215, thumbnail: 'https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg' },
          { title: 'Slow Pulp - Idaho Live', url: 'https://www.youtube.com/watch?v=def456', uploader: 'Live Room', duration: 188, thumbnail: 'https://i.ytimg.com/vi/jNQXAC9IVRw/hqdefault.jpg' }
        ]
      }});
    }
  }
};'''
    return html.replace('{{MCP_EXT_APPS_BUNDLE}}', shim).replace('{{YOUTUBE_SEARCH_APP_SCRIPT}}', script)

def write_log(step: str) -> None:
    with LOG.open('a') as f:
        f.write(step + '\n')

RUN.mkdir(parents=True, exist_ok=True)
SHOTS.mkdir(parents=True, exist_ok=True)
LOG.write_text('')
preview = RUN / 'preview.html'
preview.write_text(expanded_html())
url = preview.as_uri() + '?demo'

with sync_playwright() as p:
    browser = p.firefox.launch(headless=True)
    for width, height, name in [(1280, 1800, 'desktop'), (390, 844, 'mobile')]:
        page = browser.new_page(viewport={'width': width, 'height': height})
        page.goto(url)
        page.wait_for_timeout(500)
        shot = SHOTS / f'final_execution_1_search_{name}.png'
        page.screenshot(path=str(shot))
        write_log(f'step 1 action: captured {name} Search view at {shot}')
        page.get_by_role('button', name='Stats').click()
        page.wait_for_timeout(800)
        shot = SHOTS / f'final_execution_2_stats_{name}.png'
        page.screenshot(path=str(shot))
        write_log(f'step 2 action: captured {name} Stats view at {shot}')
        page.close()
    browser.close()
write_log('final datum: four screenshots captured for Search and Stats views')
print(RUN)
