<?php
/**
 * ip.php — 只显示客户端 IP。
 *
 * 用法: GET https://termfast.xisj.com/tools/ip.php
 * 返回: {"ip": "1.2.3.4"}
 */

header('Content-Type: application/json');

// 优先取真实 IP（处理反代场景）
$ip = $_SERVER['HTTP_X_FORWARDED_FOR'] ?? $_SERVER['HTTP_X_REAL_IP'] ?? $_SERVER['REMOTE_ADDR'] ?? '';
// X-Forwarded-For 可能含多个 IP，取第一个
if (str_contains($ip, ',')) {
    $ip = trim(explode(',', $ip)[0]);
}

echo json_encode(['ip' => $ip]);
