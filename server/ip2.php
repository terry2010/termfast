<?php
/**
 * ip2.php — 显示客户端 IP + 浏览器信息 + IP 归属地。
 *
 * 用法: GET https://termfast.xisj.com/tools/ip2.php
 * 返回: JSON，含 ip / browser / user_agent / geo
 */

require_once __DIR__ . '/lib/geoip.php';

header('Content-Type: application/json; charset=utf-8');
header('X-Content-Type-Options: nosniff');
header('X-Frame-Options: DENY');

// M-1: 优先使用 REMOTE_ADDR（不可被客户端伪造），仅在反代场景下才解析 XFF
$ip = $_SERVER['REMOTE_ADDR'] ?? '';
if ($ip === '' || $ip === '127.0.0.1') {
    $xff = $_SERVER['HTTP_X_FORWARDED_FOR'] ?? $_SERVER['HTTP_X_REAL_IP'] ?? '';
    if ($xff !== '') {
        $ip = trim(explode(',', $xff)[0]);
    }
}

$ua = $_SERVER['HTTP_USER_AGENT'] ?? '';

$result = [
    'ip' => $ip,
    'browser' => parseBrowser($ua),
    'user_agent' => $ua,
    'accept_language' => $_SERVER['HTTP_ACCEPT_LANGUAGE'] ?? '',
    'geo' => lookupGeo($ip),
];

echo json_encode($result, JSON_UNESCAPED_UNICODE | JSON_PRETTY_PRINT);

/**
 * 从 User-Agent 解析浏览器名称、版本、操作系统。
 */
function parseBrowser(string $ua): array {
    $browser = 'Unknown';
    $version = '';
    $os = 'Unknown';

    // 浏览器检测（注意顺序：Edge 基于 Chromium，需先检测）
    if (preg_match('/Edg\/([\d.]+)/', $ua, $m)) {
        $browser = 'Edge';
        $version = $m[1];
    } elseif (preg_match('/OPR\/([\d.]+)/', $ua, $m)) {
        $browser = 'Opera';
        $version = $m[1];
    } elseif (preg_match('/Chrome\/([\d.]+)/', $ua, $m)) {
        $browser = 'Chrome';
        $version = $m[1];
    } elseif (preg_match('/Firefox\/([\d.]+)/', $ua, $m)) {
        $browser = 'Firefox';
        $version = $m[1];
    } elseif (preg_match('/Safari\/([\d.]+)/', $ua, $m) && strpos($ua, 'Chrome') === false) {
        $browser = 'Safari';
        $version = $m[1];
    }

    // 操作系统检测
    if (preg_match('/Windows NT ([\d.]+)/', $ua, $m)) {
        $osMap = ['10.0' => '10/11', '6.3' => '8.1', '6.2' => '8', '6.1' => '7'];
        $os = 'Windows ' . ($osMap[$m[1]] ?? $m[1]);
    } elseif (strpos($ua, 'Mac OS X') !== false) {
        $os = 'macOS';
    } elseif (preg_match('/Android ([\d.]+)/', $ua, $m)) {
        $os = 'Android ' . $m[1];
    } elseif (strpos($ua, 'iPhone') !== false || strpos($ua, 'iPad') !== false) {
        $os = 'iOS';
    } elseif (strpos($ua, 'Linux') !== false) {
        $os = 'Linux';
    }

    // 是否移动端
    $isMobile = preg_match('/Mobile|Android|iPhone|iPad/', $ua) > 0;

    return [
        'name' => $browser,
        'version' => $version,
        'os' => $os,
        'mobile' => $isMobile,
    ];
}
