<?php
/**
 * TermFast Cloud Sync Proxy
 * 
 * 极简单文件代理服务器，3个端点：
 *   GET  ?action=auth_url&provider=baidu&redirect_uri=oob
 *   POST ?action=exchange    body: {"provider","code","redirect_uri","code_verifier"}
 *   POST ?action=refresh     body: {"provider","refresh_token"}
 *
 * 服务器持有 app_secret，App 端不持有任何 secret。
 * 服务器只参与 token 交换，不接触用户数据。
 *
 * 部署：
 *   1. 放到服务器 /var/www/html/tools/cloud-sync.php
 *   2. 配置 app_key / app_secret，二选一：
 *      a) 复制 config.local.example.php 为 config.local.php，填入值
 *         （config.local.php 已在 .gitignore，不会提交）
 *      b) 或在 Nginx/PHP-FPM 配置环境变量：
 *           fastcgi_param BAIDU_APP_KEY "xxx";
 *           fastcgi_param BAIDU_APP_SECRET "xxx";
 *           fastcgi_param DROPBOX_APP_KEY "xxx";
 *           fastcgi_param DROPBOX_APP_SECRET "xxx";
 *      环境变量优先于 config.local.php。
 *   3. 确保 HTTPS（Let's Encrypt 免费证书）
 */

// === 配置：优先环境变量，其次 server/config.local.php（不提交 git） ===
$LOCAL_CONFIG = [];
if (is_file(__DIR__ . '/config.local.php')) {
    $LOCAL_CONFIG = require __DIR__ . '/config.local.php';
}
$get_cfg = static function (string $key) use ($LOCAL_CONFIG): string {
    $env = getenv($key);
    if ($env !== false && $env !== '') {
        return $env;
    }
    return $LOCAL_CONFIG[$key] ?? '';
};
$DROPBOX_APP_KEY    = $get_cfg('DROPBOX_APP_KEY');
$DROPBOX_APP_SECRET = $get_cfg('DROPBOX_APP_SECRET');
$BAIDU_APP_KEY      = $get_cfg('BAIDU_APP_KEY');
$BAIDU_APP_SECRET   = $get_cfg('BAIDU_APP_SECRET');

// Mobile OAuth callback URL — this script receives the OAuth code from the
// provider, then redirects the browser to termfast://oauth/callback?code=...
// The Android app catches this deep link and passes the code to the FFI layer.
// Must be registered in Dropbox/Baidu developer console.
$MOBILE_CALLBACK_URL = 'https://termfast.xisj.com/tools/cloud-sync-callback.php';

// === 安全响应头 (L-2) ===
header('X-Content-Type-Options: nosniff');
header('X-Frame-Options: DENY');
header('Cache-Control: no-store');

// CORS: not needed — HTTP calls come from Rust reqwest (not browser fetch),
// so CORS does not apply. The webview never calls this server directly;
// all requests go through Tauri IPC → Rust reqwest → here.

// === 速率限制：基于客户端 IP 的滑动窗口 ===
// 每分钟最多 10 次请求，防止滥用服务器作为 OAuth 代理
function checkRateLimit(): bool {
    $ip = $_SERVER['REMOTE_ADDR'] ?? 'unknown';
    $file = sys_get_temp_dir() . '/termfast_ratelimit_' . md5($ip);
    $now = time();
    $window = 60; // 60 秒窗口
    $maxRequests = 10;

    $data = [];
    if (file_exists($file)) {
        $raw = file_get_contents($file);
        $data = array_filter(json_decode($raw, true) ?: [], fn($t) => $t > $now - $window);
    }
    if (count($data) >= $maxRequests) {
        return false;
    }
    $data[] = $now;
    file_put_contents($file, json_encode($data));
    return true;
}

// === 路由 ===
header('Content-Type: application/json; charset=utf-8');

$action = $_GET['action'] ?? '';
$method = $_SERVER['REQUEST_METHOD'];

// ping 端点不受速率限制
if ($action === 'ping' && $method === 'GET') {
    echo json_encode(['ok' => true, 'time' => time()]);
    exit;
}

// 所有其他端点检查速率限制
if (!checkRateLimit()) {
    http_response_code(429);
    echo json_encode(['error' => 'rate limit exceeded, try again later']);
    exit;
}

try {
    if ($action === 'auth_url' && $method === 'GET') {
        handleAuthUrl();
    } elseif ($action === 'exchange' && $method === 'POST') {
        handleExchange();
    } elseif ($action === 'refresh' && $method === 'POST') {
        handleRefresh();
    } else {
        http_response_code(404);
        echo json_encode(['error' => 'unknown action']);
    }
} catch (Exception $e) {
    http_response_code(500);
    echo json_encode(['error' => 'internal error']);
}

// === 端点实现 ===

/**
 * GET ?action=auth_url&provider=baidu&redirect_uri=oob
 * 
 * 返回授权 URL。App 本地生成 PKCE code_verifier（Dropbox），
 * 服务器只负责拼 URL（因为 app_key 在服务器上）。
 */
function handleAuthUrl() {
    global $DROPBOX_APP_KEY, $BAIDU_APP_KEY, $MOBILE_CALLBACK_URL;

    $provider = $_GET['provider'] ?? '';
    $redirect_uri = $_GET['redirect_uri'] ?? 'oob';

    // Whitelist provider
    if (!in_array($provider, ['dropbox', 'baidu'], true)) {
        http_response_code(400);
        echo json_encode(['error' => 'invalid provider']);
        return;
    }

    // Whitelist redirect_uri — allow oob, localhost callbacks, and the
    // mobile relay callback (cloud-sync-callback.php redirects to termfast://)
    if ($redirect_uri !== 'oob'
        && $redirect_uri !== $MOBILE_CALLBACK_URL
        && !preg_match('/^https?:\/\/localhost(:\d+)?\//', $redirect_uri)) {
        http_response_code(400);
        echo json_encode(['error' => 'invalid redirect_uri']);
        return;
    }

    if ($provider === 'dropbox') {
        // Dropbox PKCE: code_verifier 由 App 本地生成，code_challenge 由 App 算好传过来
        $code_challenge = $_GET['code_challenge'] ?? '';
        if (!$code_challenge) {
            http_response_code(400);
            echo json_encode(['error' => 'missing code_challenge for dropbox']);
            return;
        }
        $url = sprintf(
            'https://www.dropbox.com/oauth2/authorize?client_id=%s&response_type=code&code_challenge=%s&code_challenge_method=S256&token_access_type=offline&redirect_uri=%s',
            urlencode($DROPBOX_APP_KEY),
            urlencode($code_challenge),
            urlencode($redirect_uri)
        );
        echo json_encode(['auth_url' => $url, 'provider' => 'dropbox']);
        
    } elseif ($provider === 'baidu') {
        // 百度 Authorization Code flow（有 refresh_token！不再用 implicit grant）
        $state = bin2hex(random_bytes(16));
        // L-1: Store state server-side for later verification in exchange
        $stateFile = sys_get_temp_dir() . '/termfast_oauth_states.json';
        $states = [];
        if (is_file($stateFile)) {
            $states = json_decode(file_get_contents($stateFile), true) ?: [];
        }
        $states[$state] = time();
        // Prune entries older than 10 minutes
        foreach ($states as $k => $v) {
            if (time() - $v > 600) unset($states[$k]);
        }
        file_put_contents($stateFile, json_encode($states));
        $url = sprintf(
            'https://openapi.baidu.com/oauth/2.0/authorize?response_type=code&client_id=%s&redirect_uri=%s&scope=basic,netdisk&display=mobile&state=%s',
            urlencode($BAIDU_APP_KEY),
            urlencode($redirect_uri),
            $state
        );
        echo json_encode(['auth_url' => $url, 'state' => $state, 'provider' => 'baidu']);
        
    } else {
        http_response_code(400);
        echo json_encode(['error' => 'unknown provider: ' . $provider]);
    }
}

/**
 * POST ?action=exchange
 * body: {"provider":"baidu","code":"xxx","redirect_uri":"oob","code_verifier":"xxx"}
 *
 * 服务器用 app_secret + code 换 token，返回给 App。
 * 百度 Authorization Code flow 返回 access_token + refresh_token（10年有效）。
 */
function handleExchange() {
    global $DROPBOX_APP_KEY, $DROPBOX_APP_SECRET, $BAIDU_APP_KEY, $BAIDU_APP_SECRET, $MOBILE_CALLBACK_URL;
    
    $body = json_decode(file_get_contents('php://input'), true);
    if (!$body) {
        http_response_code(400);
        echo json_encode(['error' => 'invalid JSON body']);
        return;
    }
    
    $provider = $body['provider'] ?? '';
    $code = $body['code'] ?? '';
    $redirect_uri = $body['redirect_uri'] ?? 'oob';
    $code_verifier = $body['code_verifier'] ?? '';
    $state = $body['state'] ?? '';

    if (!$code) {
        http_response_code(400);
        echo json_encode(['error' => 'missing code']);
        return;
    }

    // Validate redirect_uri (H-2: was missing — only auth_url endpoint checked)
    if ($redirect_uri !== 'oob'
        && $redirect_uri !== $MOBILE_CALLBACK_URL
        && !preg_match('/^https?:\/\/localhost(:\d+)?\//', $redirect_uri)) {
        http_response_code(400);
        echo json_encode(['error' => 'invalid redirect_uri']);
        return;
    }

    // L-1: Validate OAuth state for baidu (CSRF protection)
    if ($provider === 'baidu') {
        if (!$state) {
            http_response_code(400);
            echo json_encode(['error' => 'missing state']);
            return;
        }
        $stateFile = sys_get_temp_dir() . '/termfast_oauth_states.json';
        $states = is_file($stateFile) ? (json_decode(file_get_contents($stateFile), true) ?: []) : [];
        if (!isset($states[$state])) {
            http_response_code(400);
            echo json_encode(['error' => 'invalid or expired state']);
            return;
        }
        // Consume the state (one-time use)
        unset($states[$state]);
        file_put_contents($stateFile, json_encode($states));
    }

    if ($provider === 'dropbox') {
        // Dropbox PKCE: no client_secret needed (public client)
        $params = http_build_query([
            'grant_type' => 'authorization_code',
            'code' => $code,
            'code_verifier' => $code_verifier,
            'client_id' => $DROPBOX_APP_KEY,
            'redirect_uri' => $redirect_uri,
        ]);
        echo httpPost('https://api.dropboxapi.com/oauth2/token', $params);
        
    } elseif ($provider === 'baidu') {
        // Baidu doesn't validate state server-side in token exchange,
        // but we include it for client-side CSRF checking
        $params = http_build_query([
            'grant_type' => 'authorization_code',
            'code' => $code,
            'client_id' => $BAIDU_APP_KEY,
            'client_secret' => $BAIDU_APP_SECRET,
            'redirect_uri' => $redirect_uri,
        ]);
        echo httpPost('https://openapi.baidu.com/oauth/2.0/token', $params);
        
    } else {
        http_response_code(400);
        echo json_encode(['error' => 'unknown provider: ' . $provider]);
    }
}

/**
 * POST ?action=refresh
 * body: {"provider":"baidu","refresh_token":"xxx"}
 *
 * 用 refresh_token 换新的 access_token。
 * 百度 refresh_token 有效期 10 年，可实现自动续期。
 */
function handleRefresh() {
    global $DROPBOX_APP_KEY, $DROPBOX_APP_SECRET, $BAIDU_APP_KEY, $BAIDU_APP_SECRET;
    
    $body = json_decode(file_get_contents('php://input'), true);
    if (!$body) {
        http_response_code(400);
        echo json_encode(['error' => 'invalid JSON body']);
        return;
    }
    
    $provider = $body['provider'] ?? '';
    $refresh_token = $body['refresh_token'] ?? '';
    
    if (!$refresh_token) {
        http_response_code(400);
        echo json_encode(['error' => 'missing refresh_token']);
        return;
    }
    
    if ($provider === 'dropbox') {
        // Dropbox PKCE: no client_secret needed (public client)
        $params = http_build_query([
            'grant_type' => 'refresh_token',
            'refresh_token' => $refresh_token,
            'client_id' => $DROPBOX_APP_KEY,
        ]);
        echo httpPost('https://api.dropboxapi.com/oauth2/token', $params);
        
    } elseif ($provider === 'baidu') {
        $params = http_build_query([
            'grant_type' => 'refresh_token',
            'refresh_token' => $refresh_token,
            'client_id' => $BAIDU_APP_KEY,
            'client_secret' => $BAIDU_APP_SECRET,
        ]);
        echo httpPost('https://openapi.baidu.com/oauth/2.0/token', $params);
        
    } else {
        http_response_code(400);
        echo json_encode(['error' => 'unknown provider: ' . $provider]);
    }
}

// === 工具函数 ===

/**
 * 发送 POST 请求，返回过滤后的响应体。
 * 成功时透传 token JSON（含 access_token/refresh_token）。
 * 失败时返回标准化错误，不泄露上游内部信息。
 */
function httpPost($url, $params) {
    $ch = curl_init($url);
    curl_setopt_array($ch, [
        CURLOPT_POST => true,
        CURLOPT_POSTFIELDS => $params,
        CURLOPT_RETURNTRANSFER => true,
        CURLOPT_TIMEOUT => 15,
        CURLOPT_HTTPHEADER => [
            'Content-Type: application/x-www-form-urlencoded',
            'Accept: application/json',
        ],
    ]);

    $resp = curl_exec($ch);
    $code = curl_getinfo($ch, CURLINFO_HTTP_CODE);
    $err = curl_error($ch);
    curl_close($ch);

    if ($err) {
        http_response_code(502);
        return json_encode(['error' => 'upstream connection error']);
    }

    if ($code >= 400) {
        // Parse upstream error and return standardized message
        // without leaking internal details (e.g. invalid_client, app_key format)
        $parsed = json_decode($resp, true);
        $errorType = $parsed['error'] ?? $parsed['error_description'] ?? 'unknown';
        // Map common OAuth errors to generic messages
        $safeErrors = [
            'invalid_grant' => 'authorization code or refresh token is invalid or expired',
            'invalid_client' => 'server authentication failed',
            'invalid_request' => 'malformed request',
            'unauthorized_client' => 'not authorized',
            'unsupported_grant_type' => 'unsupported operation',
        ];
        $msg = $safeErrors[$errorType] ?? 'request failed';
        // DEBUG: temporarily include raw upstream response for troubleshooting
        error_log("[cloud-sync] upstream error: code=$code, resp=$resp, errorType=$errorType");
        http_response_code($code >= 500 ? 502 : 400);
        return json_encode(['error' => $msg, 'debug_raw' => $resp, 'debug_code' => $code]);
    }

    return $resp ?: json_encode(['error' => 'empty response from upstream']);
}
