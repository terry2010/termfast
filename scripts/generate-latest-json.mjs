#!/usr/bin/env node
// Generate Tauri updater manifest (latest.json) from an existing GitHub Release.
// Usage: node scripts/generate-latest-json.mjs <tag>
// Env: GITHUB_TOKEN (required for draft releases), GITHUB_REPOSITORY
// Outputs: public/latest.json

import fs from "node:fs";
import path from "node:path";

const tag = process.argv[2] || process.env.GITHUB_REF_NAME;
if (!tag) {
  console.error("Usage: node scripts/generate-latest-json.mjs <tag>");
  process.exit(1);
}

const version = tag.replace(/^v/, "");
const repo = process.env.GITHUB_REPOSITORY || "terry2010/termfast";
const [owner, repoName] = repo.split("/");
const proxyPrefix = process.env.GH_PROXY_PREFIX || "https://gh-proxy.com/";

// Read the actual productName from tauri.conf.json so asset names match the bundle.
const tauriConf = JSON.parse(fs.readFileSync(path.join(process.cwd(), "src-tauri", "tauri.conf.json"), "utf8"));
const productName = tauriConf.productName;
const baseReleaseUrl = `https://github.com/${owner}/${repoName}/releases/download/${tag}`;

/**
 * @param {string} assetName
 */
function assetUrl(assetName) {
  const encoded = encodeURIComponent(assetName).replace(/%20/g, "%20");
  return `${proxyPrefix}${baseReleaseUrl}/${encoded}`;
}

async function githubApi(url) {
  const headers = {
    Accept: "application/vnd.github+json",
    "X-GitHub-Api-Version": "2022-11-28",
  };
  const token = process.env.GITHUB_TOKEN;
  if (token) headers.Authorization = `Bearer ${token}`;
  const res = await fetch(url, { headers });
  if (!res.ok) {
    throw new Error(`GitHub API ${url} returned ${res.status}: ${await res.text()}`);
  }
  return res.json();
}

async function downloadText(url) {
  const token = process.env.GITHUB_TOKEN;
  const headers = token ? { Authorization: `Bearer ${token}` } : {};
  const res = await fetch(url, { headers });
  if (!res.ok) {
    throw new Error(`Failed to download ${url}: ${res.status}`);
  }
  return res.text();
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function main() {
  // Draft releases are not accessible via /releases/tags/{tag}, so list all releases
  // and find the one matching our tag. Retry for up to 5 minutes in case assets
  // are still being uploaded by upstream jobs.
  const listUrl = `https://api.github.com/repos/${owner}/${repoName}/releases?per_page=100`;
  let release = null;
  for (let attempt = 0; attempt < 10; attempt++) {
    console.log(`Fetching releases list (attempt ${attempt + 1}/10)...`);
    const releases = await githubApi(listUrl);
    release = releases.find((r) => r.tag_name === tag);
    if (!release) {
      console.error(`Release with tag ${tag} not found among ${releases.length} releases.`);
      console.error("Available tags:", releases.map((r) => r.tag_name).join(", "));
      await sleep(30000);
      continue;
    }
    console.log(`Found release: ${release.tag_name} (draft=${release.draft}, assets=${release.assets.length})`);
    if (release.assets.length > 0) break;
    console.log("No assets yet, waiting 30s...");
    await sleep(30000);
  }
  if (!release) {
    console.error(`Release ${tag} never appeared.`);
    process.exit(1);
  }

  /** @type {Record<string, { signature: string; url: string }>} */
  const platforms = {};

  /**
   * @param {string} assetName
   * @param {string} platformKey
   */
  async function addPlatform(assetName, platformKey) {
    const asset = release.assets.find((a) => a.name === assetName);
    if (!asset) {
      console.warn(`Asset not found: ${assetName}`);
      return;
    }
    const sigAsset = release.assets.find((a) => a.name === `${assetName}.sig`);
    let signature = "";
    if (sigAsset) {
      signature = (await downloadText(sigAsset.browser_download_url)).trim();
    } else {
      console.error(`Signature asset not found for: ${assetName} — refusing to publish unsigned update`);
      process.exit(1);
    }
    platforms[platformKey] = {
      signature,
      url: assetUrl(assetName),
    };
    console.log(`Added ${platformKey}: ${assetName}`);
  }

  // macOS Apple Silicon — updater uses the .app.tar.gz bundle.
  // Asset name may or may not include version, try both patterns.
  await addPlatform(`${productName}_${version}_aarch64.app.tar.gz`, "darwin-aarch64");
  await addPlatform(`${productName}_aarch64.app.tar.gz`, "darwin-aarch64");

  // Windows x86_64 — NSIS installer used by installMode: basicUi.
  await addPlatform(`${productName}_${version}_x64-setup.exe`, "windows-x86_64");
  await addPlatform(`${productName}_x64-setup.exe`, "windows-x86_64");

  if (Object.keys(platforms).length === 0) {
    console.error("No update platforms could be resolved from release assets.");
    process.exit(1);
  }

  const notes = release.body || "";
  const pubDate = new Date(release.published_at || Date.now()).toISOString();

  const manifest = {
    version,
    notes,
    pub_date: pubDate,
    platforms,
  };

  const outDir = path.join(process.cwd(), "public");
  fs.mkdirSync(outDir, { recursive: true });
  const outPath = path.join(outDir, "latest.json");
  fs.writeFileSync(outPath, JSON.stringify(manifest, null, 2));
  console.log(`Wrote ${outPath}`);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
