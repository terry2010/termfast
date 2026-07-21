<?php
/**
 * 本地配置模板 — 复制为 config.local.php 并填入真实值。
 *
 * config.local.php 不会提交到 git（已在 .gitignore 中），
 * 服务器上保留此文件即可，cloud-sync.php 会自动加载。
 *
 * 部署：
 *   1. cp config.local.php.example config.local.php
 *   2. 填入下面的 App Key / App Secret
 *   3. 确保文件权限 0640（chmod 640 config.local.php）
 */

return [
    // 百度网盘开放平台：https://pan.baidu.com/union/
    'BAIDU_APP_KEY'    => '',
    'BAIDU_APP_SECRET' => '',

    // Dropbox Developer Console：https://www.dropbox.com/developers/apps
    'DROPBOX_APP_KEY'    => '',
    'DROPBOX_APP_SECRET' => '',
];
