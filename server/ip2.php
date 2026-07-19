<?php
/**
 * ip2.php — 显示客户端 IP + 浏览器信息 + IP 归属地。
 *
 * 用法: GET https://termfast.xisj.com/tools/ip2.php
 * 返回: JSON，含 ip / browser / user_agent / geo
 */

require_once __DIR__ . '/lib/geoip.php';

header('Content-Type: application/json; charset=utf-8');

// 优先取真实 IP（处理反代场景）
$ip = $_SERVER['HTTP_X_FORWARDED_FOR'] ?? $_SERVER['HTTP_X_REAL_IP'] ?? $_SERVER['REMOTE_ADDR'] ?? '';
if (str_contains($ip, ',')) {
    $ip = trim(explode(',', $ip)[0]);
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
    } elseif (preg_match('/Safari\/([\d.]+)/', $ua, $m) && !str_contains($ua, 'Chrome')) {
        $browser = 'Safari';
        $version = $m[1];
    }

    // 操作系统检测
    if (preg_match('/Windows NT ([\d.]+)/', $ua, $m)) {
        $osMap = ['10.0' => '10/11', '6.3' => '8.1', '6.2' => '8', '6.1' => '7'];
        $os = 'Windows ' . ($osMap[$m[1]] ?? $m[1]);
    } elseif (str_contains($ua, 'Mac OS X')) {
        $os = 'macOS';
    } elseif (preg_match('/Android ([\d.]+)/', $ua, $m)) {
        $os = 'Android ' . $m[1];
    } elseif (str_contains($ua, 'iPhone') || str_contains($ua, 'iPad')) {
        $os = 'iOS';
    } elseif (str_contains($ua, 'Linux')) {
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
