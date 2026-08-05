package com.termfast.app

/**
 * JNI bridge to the Rust `termfast_android_ffi` native library.
 *
 * All methods are thin wrappers around native functions; the actual
 * business logic lives in `crates/android-ffi`.
 */
object RustBridge {
    private var loaded = false

    fun ensureLoaded() {
        if (!loaded) {
            System.loadLibrary("termfast_android_ffi")
            nativeInit()
            // M-1: Set Rust log level based on BuildConfig.DEBUG
            nativeSetLogLevel(if (BuildConfig.DEBUG) 4 else 2) // Debug=4, Warn=2
            loaded = true
        }
    }

    external fun nativeInit()
    external fun nativeSetLogLevel(level: Int)
    external fun nativePing(): Int

    // --- Config ---
    external fun nativeSetDataDir(path: String)
    external fun nativeGetConfigJson(): String
    external fun nativeSaveConfigJson(json: String): Boolean

    // --- Server lifecycle ---
    external fun nativeAddServer(json: String): String
    external fun nativeUpdateServer(json: String): Boolean
    external fun nativeRemoveServer(id: String): Boolean
    external fun nativeListServers(): String
    external fun nativeConnectServer(id: String): Boolean
    external fun nativeDisconnectServer(id: String): Boolean
    external fun nativeGetServerStatus(id: String): String

    // --- Proxy ---
    external fun nativeStartProxy(id: String, socks5Port: Int, httpPort: Int, mixedPort: Int): Boolean
    external fun nativeStopProxy(id: String): Boolean
    external fun nativeIsProxyRunning(id: String): Boolean

    // --- VPN ---
    external fun nativeStartVpn(id: String, tunFd: Int, mtu: Int, socks5Port: Int, dnsStrategy: String, ipv6Enabled: Boolean): Boolean
    external fun nativeStopVpn(id: String): Boolean
    external fun nativeGetLastError(): String
    external fun nativeGetLastErrorCode(): String
    external fun nativeGetLastErrorRaw(): String
    external fun nativeAcceptHostKey(id: String, fingerprint: String): Boolean

    // --- Triggers ---
    external fun nativeListTriggers(serverId: String): String
    external fun nativeListTriggerTemplates(): String
    external fun nativeSetTriggerTemplates(json: String): Boolean
    external fun nativeSetServerTriggers(serverId: String, json: String): Boolean
    external fun nativeRunTrigger(serverId: String, triggerId: String): String

    // --- Key generation ---
    external fun nativeGenerateKeypair(serverId: String): String

    // --- Event subscription ---
    external fun nativeSetEventListener(listener: RustEventListener)

    // --- Socket protect (VpnService.protect) ---
    external fun nativeSetProtectCallback(vpnService: android.net.VpnService)
    external fun nativeClearProtectCallback()

    // --- Credential ---
    external fun nativeSaveCredential(serverId: String, type: String, value: String): Boolean
    external fun nativeLoadCredential(serverId: String, type: String): String?
    external fun nativeDeleteCredential(serverId: String, type: String): Boolean

    // --- Credential encryption management ---
    external fun nativeCredentialStatus(): String
    external fun nativeCredentialInitialize(masterPassword: String): Boolean
    external fun nativeCredentialUnlock(masterPassword: String): Boolean
    external fun nativeCredentialUnlockWithKey(keyBase64: String): Boolean
    external fun nativeCredentialGetKey(): String?
    external fun nativeCredentialLock()
    external fun nativeCredentialMigrate(masterPassword: String): Boolean
    external fun nativeCredentialChangePassword(oldPassword: String, newPassword: String): Boolean
    external fun nativeCredentialReset(): Boolean
    external fun nativeCredentialExport(destPath: String): Boolean
    external fun nativeCredentialImport(srcPath: String, masterPassword: String): Boolean
    external fun nativeCredentialIsUnlocked(): Boolean

    // --- SSH Terminal (PTY) ---
    external fun nativeOpenTerminal(serverId: String, sessionId: String, cols: Int, rows: Int): Boolean
    external fun nativeWriteTerminal(sessionId: String, data: String): Boolean
    external fun nativeCloseTerminal(sessionId: String): Boolean
    external fun nativeResizeTerminal(sessionId: String, cols: Int, rows: Int): Boolean

    // --- tmux Session Management ---
    external fun nativeTmuxListSessions(serverId: String): String
    external fun nativeTmuxNewSession(serverId: String, sessionId: String, description: String, cols: Int, rows: Int): String
    external fun nativeTmuxAttachSession(serverId: String, sessionId: String, tmuxSessionName: String, cols: Int, rows: Int): String
    external fun nativeTmuxKillSession(serverId: String, tmuxSessionName: String): Boolean

    // --- Pairing (HTTP client to Go backend) ---
    external fun nativePairingRegister(email: String, password: String): String
    external fun nativePairingLogin(email: String, password: String): String
    external fun nativePairingComplete(pairingId: String, phonePubkey: String, deviceId: String): String
    external fun nativePairingDownloadConfig(pairingJwt: String): String

    // --- Remote Terminal (WebSocket tunnel frame-level API) ---
    // Kotlin TunnelClient manages WebSocket; Rust FFI handles frame crypto + protocol.
    external fun nativeRemoteTunnelInit(pairingId: String, pairingKey: ByteArray): ByteArray
    external fun nativeRemoteTunnelOnBinary(pairingId: String, data: ByteArray)
    external fun nativeRemoteTunnelSendListRequest(pairingId: String): ByteArray?
    external fun nativeRemoteTunnelSubscribe(pairingId: String, terminalId: Int): ByteArray?
    external fun nativeRemoteTunnelUnsubscribe(pairingId: String, terminalId: Int): ByteArray?
    external fun nativeRemoteTunnelSendInput(pairingId: String, terminalId: Int, data: ByteArray): ByteArray?
    external fun nativeRemoteTunnelSendResize(pairingId: String, terminalId: Int, cols: Int, rows: Int): ByteArray?
    external fun nativeRemoteTunnelClose(pairingId: String): ByteArray?

    // --- Cloud Sync ---
    external fun nativeCloudSyncAuthUrl(provider: String): String
    external fun nativeCloudSyncExchangeCode(code: String): String
    external fun nativeCloudSyncSaveToken(tokenJson: String): Boolean
    external fun nativeCloudSyncLoadToken(provider: String): String
    external fun nativeCloudSyncUpload(paramsJson: String): String
    external fun nativeCloudSyncDownload(paramsJson: String): String
    external fun nativeCloudSyncStatus(provider: String): String
    external fun nativeCloudSyncDisconnect(provider: String): Boolean

    // Port forwarding (PF-7)
    external fun nativeListPortForwards(serverId: String): String
    external fun nativeAddPortForward(serverId: String, ruleJson: String): String
    external fun nativeUpdatePortForward(serverId: String, ruleId: String, ruleJson: String): String
    external fun nativeDeletePortForward(serverId: String, ruleId: String): String
    external fun nativeStartPortForward(serverId: String, ruleId: String): String
    external fun nativeStopPortForward(serverId: String, ruleId: String): String
}
