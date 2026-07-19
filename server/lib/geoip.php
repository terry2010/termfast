<?php
/**
 * GeoIP 查询共用函数 — 基于 MaxMind GeoLite2-City 数据库。
 *
 * 依赖: composer require maxmind-db/reader
 * 数据库: server/data/GeoLite2-City.mmdb
 *
 * 提供两个函数:
 *   lookupGeo($ip) — 返回国家/省/市/经纬度/时区
 *   isCN($ip)      — 返回是否中国 IP
 */

require_once __DIR__ . '/../vendor/autoload.php';

use MaxMind\Db\Reader;

/**
 * 查询 IP 的地理位置信息。
 *
 * @param string $ip 客户端 IP
 * @return array {country, country_code, province, city, latitude, longitude, timezone}
 *               查询失败时包含 error 字段
 */
function lookupGeo(string $ip): array {
    $mmdbPath = __DIR__ . '/../data/GeoLite2-City.mmdb';
    if (!file_exists($mmdbPath)) {
        return ['error' => 'geoip database not found'];
    }
    try {
        $reader = new Reader($mmdbPath);
        $record = $reader->get($ip);
        $reader->close();
        if (!$record) {
            return ['error' => 'not found in database'];
        }
        return [
            'country' => $record['country']['names']['zh-CN']
                ?? $record['country']['names']['en']
                ?? '',
            'country_code' => $record['country']['iso_code'] ?? '',
            'province' => $record['subdivisions'][0]['names']['zh-CN']
                ?? $record['subdivisions'][0]['names']['en']
                ?? '',
            'city' => $record['city']['names']['zh-CN']
                ?? $record['city']['names']['en']
                ?? '',
            'latitude' => $record['location']['latitude'] ?? null,
            'longitude' => $record['location']['longitude'] ?? null,
            'timezone' => $record['location']['time_zone'] ?? '',
        ];
    } catch (Exception $e) {
        return ['error' => $e->getMessage()];
    }
}

/**
 * 判断 IP 是否来自中国。
 *
 * @param string $ip 客户端 IP
 * @return bool
 */
function isCN(string $ip): bool {
    // 本地/内网 IP 默认当国内（开发环境）
    if ($ip === '127.0.0.1' || str_starts_with($ip, '192.168.') || str_starts_with($ip, '10.')) {
        return true;
    }
    $geo = lookupGeo($ip);
    return ($geo['country_code'] ?? '') === 'CN';
}
