<?php
/**
 * latest.php — Tauri 更新清单 + IP 智能选源。
 *
 * 用法: GET https://termfast.xisj.com/tools/latest.php
 *
 * 流程:
 *   1. 检查本地缓存（5 分钟）
 *   2. 缓存过期 → 调 GitHub API 获取最新 Release
 *   3. 根据客户端 IP 判断国内外
 *      国内 → 下载 URL 用 termfast.xisj.com/releases/（自建反代）
 *      海外 → 下载 URL 用 github.com 直连
 *   4. 返回 Tauri updater 格式的 latest.json
 *
 * 零维护: 只需在 GitHub 打 tag 发 Release，PHP 自动抓取。
 */

require_once __DIR__ . '/lib/geoip.php';

header('Content-Type: application/json; charset=utf-8');

// === 配置 ===
$repo = 'terry2010/termfast';
$cacheFile = sys_get_temp_dir() . '/termfast_latest_cache.json';
$cacheTtl = 300; // 5 分钟缓存
$proxyBase = 'https://termfast.xisj.com/releases/';
$githubBase = "https://github.com/{$repo}/releases/download/";

// === 获取客户端 IP ===
$ip = $_SERVER['HTTP_X_FORWARDED_FOR'] ?? $_SERVER['HTTP_X_REAL_IP'] ?? $_SERVER['REMOTE_ADDR'] ?? '';
if (str_contains($ip, ',')) {
    $ip = trim(explode(',', $ip)[0]);
}
$useProxy = isCN($ip);

// === 获取 Release 数据（带缓存）===
$release = fetchRelease($repo, $cacheFile, $cacheTtl);
if (!$release) {
    http_response_code(502);
    echo json_encode(['error' => 'failed to fetch release info from GitHub']);
    exit;
}

// === 构建 Tauri updater manifest ===
$manifest = buildManifest($release, $useProxy, $proxyBase, $githubBase);

if (empty($manifest['platforms'])) {
    http_response_code(404);
    echo json_encode(['error' => 'no signed platform assets found in latest release']);
    exit;
}

echo json_encode($manifest, JSON_UNESCAPED_SLASHES);

// === 函数 ===

/**
 * 从 GitHub API 获取最新 Release，带文件缓存。
 */
function fetchRelease(string $repo, string $cacheFile, int $cacheTtl): ?array {
    // 尝试读缓存
    if (file_exists($cacheFile) && (time() - filemtime($cacheFile)) < $cacheTtl) {
        $cached = json_decode(file_get_contents($cacheFile), true);
        if ($cached) return $cached;
    }

    // 调 GitHub API
    $url = "https://api.github.com/repos/{$repo}/releases/latest";
    $ch = curl_init($url);
    curl_setopt_array($ch, [
        CURLOPT_RETURNTRANSFER => true,
        CURLOPT_TIMEOUT => 10,
        CURLOPT_HTTPHEADER => [
            'Accept: application/vnd.github+json',
            'User-Agent: TermFast-Updater',
        ],
    ]);
    $resp = curl_exec($ch);
    $code = curl_getinfo($ch, CURLINFO_HTTP_CODE);
    curl_close($ch);

    if ($code !== 200 || !$resp) {
        // GitHub API 失败时，尝试用过期缓存（总比没有好）
        if (file_exists($cacheFile)) {
            $cached = json_decode(file_get_contents($cacheFile), true);
            if ($cached) return $cached;
        }
        return null;
    }

    $release = json_decode($resp, true);
    if (!$release) return null;

    // 写缓存
    file_put_contents($cacheFile, $resp);

    return $release;
}

/**
 * 构建 Tauri updater manifest。
 * 根据客户端 IP 选择下载 URL（国内反代 / 海外直连）。
 */
function buildManifest(array $release, bool $useProxy, string $proxyBase, string $githubBase): array {
    $version = ltrim($release['tag_name'] ?? '', 'v');
    $notes = $release['body'] ?? '';
    $pubDate = $release['published_at'] ?? date('c');
    $assets = $release['assets'] ?? [];

    // 建立 asset 名 → asset 的索引
    $assetMap = [];
    foreach ($assets as $asset) {
        $assetMap[$asset['name']] = $asset;
    }

    $platforms = [];

    // macOS Apple Silicon — .app.tar.gz
    $macAsset = findAsset($assetMap, $version, ['.app.tar.gz']);
    if ($macAsset) {
        $sig = fetchSignature($assetMap, $macAsset['name']);
        if ($sig) {
            $platforms['darwin-aarch64'] = [
                'signature' => $sig,
                'url' => assetUrl($macAsset['name'], $useProxy, $proxyBase, $githubBase),
            ];
        }
    }

    // Windows x86_64 — .exe setup
    $winAsset = findAsset($assetMap, $version, ['_x64-setup.exe', '-x64-setup.exe']);
    if ($winAsset) {
        $sig = fetchSignature($assetMap, $winAsset['name']);
        if ($sig) {
            $platforms['windows-x86_64'] = [
                'signature' => $sig,
                'url' => assetUrl($winAsset['name'], $useProxy, $proxyBase, $githubBase),
            ];
        }
    }

    return [
        'version' => $version,
        'notes' => $notes,
        'pub_date' => $pubDate,
        'platforms' => $platforms,
    ];
}

/**
 * 在 asset 列表中查找匹配的平台包（尝试带版本号和不带版本号两种命名）。
 */
function findAsset(array $assetMap, string $version, array $suffixes): ?array {
    // 尝试常见的产品名前缀
    $names = ['TermFast', 'termfast'];
    foreach ($names as $name) {
        foreach ($suffixes as $suffix) {
            // 带版本号: TermFast_0.2.6_aarch64.app.tar.gz
            $key = "{$name}_{$version}{$suffix}";
            if (isset($assetMap[$key])) return $assetMap[$key];
            // 不带版本号: TermFast_aarch64.app.tar.gz
            $key = "{$name}{$suffix}";
            if (isset($assetMap[$key])) return $assetMap[$key];
        }
    }
    // 兜底：遍历所有 asset 找后缀匹配
    foreach ($assetMap as $name => $asset) {
        foreach ($suffixes as $suffix) {
            if (str_ends_with($name, $suffix)) return $asset;
        }
    }
    return null;
}

/**
 * 获取签名文件内容。签名缺失返回 null（不发布无签名更新）。
 */
function fetchSignature(array $assetMap, string $assetName): ?string {
    $sigName = $assetName . '.sig';
    if (!isset($assetMap[$sigName])) return null;

    $sig = @file_get_contents($assetMap[$sigName]['browser_download_url']);
    if (!$sig) return null;

    return trim($sig);
}

/**
 * 根据客户端 IP 选择下载 URL。
 */
function assetUrl(string $assetName, bool $useProxy, string $proxyBase, string $githubBase): string {
    $encoded = urlencode($assetName);
    if ($useProxy) {
        return $proxyBase . $encoded;
    }
    return $githubBase . $encoded;
}
