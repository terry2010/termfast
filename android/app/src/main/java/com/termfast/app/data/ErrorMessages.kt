package com.termfast.app.data

/**
 * Parse backend ErrorCode + English detail into user-friendly Chinese messages.
 *
 * Mirrors the frontend's localizeDetail() in useIpc.ts.
 */
object ErrorMessages {

    fun format(code: String?, detail: String?): String {
        if (code == null) return detail ?: "未知错误"
        val d = (detail ?: "").lowercase()

        return when (code) {
            "SshConnectFailed" -> when {
                d.contains("timed out") || d.contains("timeout") ->
                    "连接超时，请检查服务器地址和端口是否正确，以及网络是否畅通"
                d.contains("connection refused") ->
                    "服务器拒绝连接，可能是 SSH 服务未启动或端口号错误"
                d.contains("unreachable") || d.contains("noroutetohost") ->
                    "网络不可达，请检查本地网络或 VPN 是否正常"
                d.contains("dns") || d.contains("name or service not known") ->
                    "域名解析失败，请检查服务器地址是否正确"
                d.contains("reset") || d.contains("broken pipe") ->
                    "连接被重置，可能是网络不稳定或服务器主动断开"
                d.contains("banner") || d.contains("protocol") ->
                    "SSH 协议错误，服务器可能不是标准 SSH 服务"
                else -> "无法连接到服务器，请检查地址和端口"
            }

            "AuthFailed" -> when {
                d.contains("rejected by server") ->
                    "用户名或密码错误，请重新输入"
                d.contains("key file not found") ->
                    "密钥文件不存在，请检查密钥路径"
                d.contains("failed to load key") ->
                    "密钥加载失败，可能是文件格式错误或密码短语不正确"
                d.contains("password auth error") ->
                    "用户名或密码错误，请重新输入"
                d.contains("key auth error") ->
                    "密钥认证失败，请检查密钥配置"
                else -> "用户名或密码错误，请重新输入"
            }

            "HostKeyMismatch" ->
                "服务器主机密钥已变更，可能服务器重装了系统，请确认安全后重新连接"

            "CredentialNotFound" -> when {
                d.contains("key file") -> "密钥文件不存在，请检查密钥路径"
                else -> "未找到保存的凭据，请重新输入密码"
            }

            "PortConflict", "ProxyPortInUse" ->
                "端口 $detail 已被其他程序占用，请更换端口"

            "NeedsPrivilege" ->
                "需要管理员权限才能修改系统代理设置"

            "SshDisconnected" -> when {
                d.contains("reset") || d.contains("broken pipe") ->
                    "连接被重置，可能是网络不稳定或服务器主动断开"
                d.contains("timeout") || d.contains("timed out") ->
                    "连接超时，网络可能不稳定"
                else -> "连接已断开"
            }

            "ConfigCorrupt" -> "配置文件损坏：$detail"
            "ConfigMigrationFailed" -> "配置迁移失败：$detail"
            "CredentialStoreFailed" -> "凭据存储失败：$detail"
            "TemplateNotFound" -> "模板未找到"
            "TriggerNotFound" -> "触发器未找到"
            "ServerNotFound" -> "服务器未找到"
            "ImportFailed" -> "导入失败：$detail"
            "DecryptionFailed" -> "解密失败：$detail"
            "TriggerCommandFailed" -> "触发器命令执行失败：$detail"
            "Internal" -> "内部错误：${detail ?: "未知错误"}"
            "InvalidParams" -> "参数错误：$detail"
            else -> detail ?: "未知错误"
        }
    }
}
