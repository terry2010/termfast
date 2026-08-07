// Clear WebView2 disk cache before app starts (Windows dev mode only).
// This prevents stale cached JS modules from masking HMR updates.
const fs = require("fs");
const path = require("path");

const localAppData = process.env.LOCALAPPDATA;
if (!localAppData) {
  // Not Windows, nothing to do
  process.exit(0);
}

const cacheRoot = path.join(localAppData, "com.termfast.app", "EBWebView", "Default");
const targets = [
  path.join(cacheRoot, "Cache", "Cache_Data"),
  path.join(cacheRoot, "Code Cache", "js"),
  path.join(cacheRoot, "Code Cache", "wasm"),
];

for (const p of targets) {
  if (fs.existsSync(p)) {
    try {
      fs.rmSync(p, { recursive: true, force: true });
      console.log(`[clear-webview-cache] removed ${p}`);
    } catch (e) {
      // Non-fatal — cache may be locked by a running instance
      console.warn(`[clear-webview-cache] could not remove ${p}: ${e.message}`);
    }
  }
}
