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
            loaded = true
        }
    }

    external fun nativeInit()
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
}
