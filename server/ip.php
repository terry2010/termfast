<?php
/**
 * ip.php — 只显示客户端 IP。
 *
 * 用法: GET https://termfast.xisj.com/tools/ip.php
 * 返回: {"ip": "1.2.3.4"}
 */

header('Content-Type: application/json');
header('X-Content-Type-Options: nosniff');
header('X-Frame-Options: DENY');

// M-1: 优先使用 REMOTE_ADDR（不可被客户端伪造），仅在反代场景下才解析 XFF
$ip = $_SERVER['REMOTE_ADDR'] ?? '';
if ($ip === '' || $ip === '127.0.0.1') {
    // Behind reverse proxy — parse XFF but only trust the first hop
    $xff = $_SERVER['HTTP_X_FORWARDED_FOR'] ?? $_SERVER['HTTP_X_REAL_IP'] ?? '';
    if ($xff !== '') {
        $ip = trim(explode(',', $xff)[0]);
    }
}

echo json_encode(['ip' => $ip]);
