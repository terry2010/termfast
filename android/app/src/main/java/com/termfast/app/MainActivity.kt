package com.termfast.app

import android.Manifest
import android.content.pm.PackageManager
import android.net.VpnService
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.ui.Modifier
import androidx.core.content.ContextCompat
import com.termfast.app.data.CloudSyncManager
import com.termfast.app.data.CredentialManager
import com.termfast.app.data.ErrorMessages
import com.termfast.app.data.PairingStore
import com.termfast.app.data.RustEvent
import com.termfast.app.data.RustRepository
import com.termfast.app.data.SettingsRepository
import com.termfast.app.service.NotificationHelper
import com.termfast.app.service.SshVpnService
import com.termfast.app.service.SshVpnTileService
import com.termfast.app.ui.TermFastApp
import com.termfast.app.ui.theme.TermFastTheme
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch

class MainActivity : ComponentActivity() {

    private val vpnLauncher = registerForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.StartActivityForResult()
    ) { result ->
        if (result.resultCode == RESULT_OK) {
            intent?.getStringExtra("server_id")?.let { serverId ->
                val settings = SettingsRepository(this).load()
                SshVpnService.start(this, serverId, settings)
                SshVpnTileService.setLastServerId(this, serverId)
            }
        }
    }

    private val notifPermissionLauncher = registerForActivityResult(
        ActivityResultContracts.RequestPermission()
    ) { _ -> }

    private var eventCollectorJob: Job? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        RustBridge.ensureLoaded()
        // Security: set hw_id (ANDROID_ID) before any credential operation
        RustBridge.setHwId(this)
        val dataDir = filesDir.absolutePath
        RustRepository.init(dataDir)
        RustRepository.initOrdering(this)
        PairingStore.init(this)
        CloudSyncManager.appContext = applicationContext
        // Initialize TerminalSessionManager with app context (for RemoteTunnelService start/stop)
        com.termfast.app.ui.screen.TerminalSessionManager.init(this)
        // Start global terminal session event collector
        com.termfast.app.ui.screen.TerminalSessionManager.startGlobalCollector()
        // Try auto-unlock with cached derived key (no user prompt).
        // Run on IO dispatcher to avoid ANR — Argon2id key derivation
        // (32 MiB memory) can take 200-500ms on low-end devices.
        CoroutineScope(Dispatchers.IO).launch {
            val ok = CredentialManager.tryCachedUnlock(this@MainActivity)
            if (BuildConfig.DEBUG) android.util.Log.i("MainActivity", "tryCachedUnlock result: $ok, isUnlocked: ${CredentialManager.isUnlocked()}")
        }
        NotificationHelper.ensureChannels(this)
        requestNotificationPermission()
        handleStartVpnIntent(intent)
        handleOAuthDeepLink(intent)
        handleAgentApprovalIntent(intent)
        handleInjectTextIntent(intent)
        enableEdgeToEdge(
            statusBarStyle = SystemBarStyle.auto(
                android.graphics.Color.TRANSPARENT,
                android.graphics.Color.TRANSPARENT,
            ),
            navigationBarStyle = SystemBarStyle.auto(
                android.graphics.Color.TRANSPARENT,
                android.graphics.Color.TRANSPARENT,
            ),
        )
        setContent {
            TermFastTheme {
                Surface(
                    modifier = Modifier.fillMaxSize(),
                    color = MaterialTheme.colorScheme.background
                ) {
                    TermFastApp()
                }
            }
        }
    }

    override fun onResume() {
        super.onResume()
        startEventCollector()
    }

    override fun onPause() {
        super.onPause()
        // Keep collector running in background to send notifications
    }

    private fun requestNotificationPermission() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            val granted = ContextCompat.checkSelfPermission(
                this, Manifest.permission.POST_NOTIFICATIONS
            ) == PackageManager.PERMISSION_GRANTED
            if (!granted) {
                notifPermissionLauncher.launch(Manifest.permission.POST_NOTIFICATIONS)
            }
        }
    }

    private fun startEventCollector() {
        eventCollectorJob?.cancel()
        eventCollectorJob = CoroutineScope(Dispatchers.Main).launch {
            RustRepository.events.collect { event ->
                handleEvent(event)
            }
        }
    }

    private fun handleEvent(event: RustEvent) {
        val settings = SettingsRepository(this).load()
        when (event) {
            is RustEvent.ServerStatusChanged -> {
                when (event.status) {
                    "connected" -> {
                        if (settings.notify_connect_success) {
                            val msg = if (event.exit_ip != null) {
                                "已连接，出口 IP: ${event.exit_ip}"
                            } else {
                                "已连接"
                            }
                            NotificationHelper.sendEventNotification(
                                this, NotificationHelper.NOTIF_CONNECT_SUCCESS,
                                "TermFast 连接成功", msg
                            )
                        }
                    }
                    "disconnected" -> {
                        if (settings.notify_disconnect) {
                            NotificationHelper.sendEventNotification(
                                this, NotificationHelper.NOTIF_DISCONNECT,
                                "TermFast 已断开", "SSH 连接已断开"
                            )
                        }
                    }
                    "auth_failed", "offline" -> {
                        if (settings.notify_auth_fail && event.error_code != null) {
                            val msg = ErrorMessages.format(event.error_code, event.error_detail)
                            NotificationHelper.sendEventNotification(
                                this, NotificationHelper.NOTIF_AUTH_FAIL,
                                "TermFast 连接失败", msg
                            )
                        }
                    }
                }
            }
            is RustEvent.ProxyStatusChanged -> {
                if (settings.notify_proxy_toggle) {
                    val msg = if (event.proxy_running) "代理已启动" else "代理已停止"
                    NotificationHelper.sendEventNotification(
                        this, 1010, "TermFast 代理状态", msg
                    )
                }
            }
            is RustEvent.VpnStatusChanged -> {
                // VPN status handled by foreground service notification
            }
            is RustEvent.IpChanged -> {
                if (settings.notify_ip_change) {
                    val msg = if (event.old_ip != null) {
                        "${event.old_ip} → ${event.new_ip}"
                    } else {
                        "新 IP: ${event.new_ip}"
                    }
                    NotificationHelper.sendEventNotification(
                        this, NotificationHelper.NOTIF_IP_CHANGE,
                        "IP 变化: ${event.server_name}", msg
                    )
                }
            }
            is RustEvent.LogEntry -> {
                // Log entries are handled by LogScreen
            }
            is RustEvent.TerminalData -> {
                // Terminal data is handled by TerminalScreen
            }
            is RustEvent.TerminalClosed -> {
                // Terminal closed is handled by TerminalScreen
            }
            is RustEvent.TerminalError -> {
                // Terminal error is handled by TerminalScreen
            }
            // Remote terminal events are handled by RemoteTerminalListScreen
            // and TerminalSessionManager's global collector.
            is RustEvent.RemoteTunnelReady,
            is RustEvent.RemoteTerminalList,
            is RustEvent.RemoteTerminalOutput,
            is RustEvent.RemoteTerminalHistory,
            is RustEvent.RemoteTerminalResize,
            is RustEvent.RemoteTerminalError,
            is RustEvent.RemoteTerminalNotify,
            is RustEvent.RemoteTerminalOk,
            is RustEvent.RemoteTriggerList,
            is RustEvent.RemoteTriggerExecResult -> { }
        }
    }

    override fun onNewIntent(intent: android.content.Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        handleStartVpnIntent(intent)
        handleOAuthDeepLink(intent)
        handleAgentApprovalIntent(intent)
        handleInjectTextIntent(intent)
    }

    /**
     * Debug-only: handle text injection from ADB.
     * Usage: adb shell am start -n com.termfast.app/.MainActivity --es inject_text "echo hi" --ez enter true
     *
     * In release builds, BuildConfig.DEBUG is always false, so this is a no-op.
     * R8 with isMinifyEnabled=true will strip the entire branch as dead code.
     */
    private fun handleInjectTextIntent(intent: android.content.Intent) {
        if (!BuildConfig.DEBUG) return
        val text = intent.getStringExtra("inject_text")
        android.util.Log.i("MainActivity", "INJECT_TEXT: checking intent, inject_text=$text, extras=${intent.extras}")
        if (text.isNullOrEmpty()) return
        val appendEnter = intent.getBooleanExtra("enter", false)
        val targetSessionId = intent.getStringExtra("session")
        // Decode escape sequences: \n \r \t \xNN \e (ESC) \a (BEL)
        val decoded = decodeEscapes(text)
        val payload = if (appendEnter) "$decoded\r" else decoded

        val sessions = com.termfast.app.ui.screen.TerminalSessionManager.getAllSessions()
        android.util.Log.i("MainActivity", "INJECT_TEXT: found ${sessions.size} sessions: ${sessions.map { it.sessionId to it.connected }}")

        // Try via TerminalSessionManager first (works for SSH + remote with registered sessions)
        if (sessions.isNotEmpty()) {
            val sessionId = if (targetSessionId != null) {
                sessions.find { it.sessionId == targetSessionId }?.sessionId
            } else {
                val connected = sessions.filter { it.connected }
                (if (connected.isNotEmpty()) connected.last() else sessions.last()).sessionId
            }
            if (sessionId != null) {
                android.util.Log.i("MainActivity", "INJECT_TEXT: ${payload.length} chars → session $sessionId")
                com.termfast.app.ui.screen.TerminalSessionManager.writeToSession(sessionId, payload)
                return
            }
        }

        // Fallback: find any active tunnel manager and send directly
        val tunnelManager = com.termfast.app.ui.screen.TerminalSessionManager.getActiveTunnelManager()
        if (tunnelManager != null) {
            val terminalId = com.termfast.app.ui.screen.TerminalSessionManager.getActiveRemoteTerminalId()
            android.util.Log.i("MainActivity", "INJECT_TEXT: fallback via tunnel manager, terminalId=$terminalId")
            val sent = tunnelManager.sendInput(terminalId, payload.toByteArray())
            android.util.Log.i("MainActivity", "INJECT_TEXT: tunnel sendInput result=$sent")
            return
        }

        android.util.Log.w("MainActivity", "INJECT_TEXT: no active session or tunnel manager")
    }

    /**
     * Decode escape sequences in the injected text.
     * Supported: \n \r \t \e (ESC) \a (BEL) \xNN (hex byte) \\ (literal backslash)
     */
    private fun decodeEscapes(s: String): String {
        val sb = StringBuilder()
        var i = 0
        while (i < s.length) {
            val c = s[i]
            if (c == '\\' && i + 1 < s.length) {
                when (s[i + 1]) {
                    'n' -> { sb.append('\n'); i += 2 }
                    'r' -> { sb.append('\r'); i += 2 }
                    't' -> { sb.append('\t'); i += 2 }
                    'e' -> { sb.append(0x1B.toChar()); i += 2 }
                    'a' -> { sb.append(0x07.toChar()); i += 2 }
                    '\\' -> { sb.append('\\'); i += 2 }
                    'x' -> {
                        if (i + 3 < s.length) {
                            val hex = s.substring(i + 2, i + 4)
                            try {
                                sb.append(hex.toInt(16).toChar())
                                i += 4
                            } catch (e: NumberFormatException) {
                                sb.append(c); i++
                            }
                        } else {
                            sb.append(c); i++
                        }
                    }
                    else -> { sb.append(c); i++ }
                }
            } else {
                sb.append(c); i++
            }
        }
        return sb.toString()
    }

    private fun handleStartVpnIntent(intent: android.content.Intent) {
        if (intent.getBooleanExtra("start_vpn", false)) {
            // M-2: Reject start_vpn from external apps — only accept from launcher (system) or self.
            // getCallingPackage() is unreliable for activity intents, so we check the action:
            // our own TileService uses a specific action; third-party startActivity won't set it.
            val action = intent.action
            val isLauncher = action == android.content.Intent.ACTION_MAIN
            val isOwnAction = action == "com.termfast.app.START_VPN"
            if (!isLauncher && !isOwnAction) {
                android.util.Log.w("MainActivity", "Rejected start_vpn with action=$action")
                return
            }
            val serverId = intent.getStringExtra("server_id") ?: return
            val prepare = VpnService.prepare(this)
            if (prepare != null) {
                vpnLauncher.launch(prepare)
            } else {
                val settings = SettingsRepository(this).load()
                SshVpnService.start(this, serverId, settings)
                SshVpnTileService.setLastServerId(this, serverId)
            }
        }
    }

    /**
     * Handle the OAuth deep link callback (termfast://oauth/callback?code=...).
     * Delegates to CloudSyncManager which exchanges the code and saves the token.
     */
    private fun handleOAuthDeepLink(intent: android.content.Intent) {
        val uri = intent.data ?: return
        // Only handle termfast://oauth/callback
        if (uri.scheme != "termfast" || uri.host != "oauth") return
        CoroutineScope(Dispatchers.IO).launch {
            CloudSyncManager.handleDeepLink(uri)
        }
    }

    /**
     * Handle "AI needs input" notification tap — navigate to AgentApprovalScreen.
     * The notification Intent carries:
     * - navigate_to: "agentApproval/{questionId}"
     * - cli: CLI type (for fallback rendering if cache is lost)
     * - question: question text (for fallback rendering)
     */
    private fun handleAgentApprovalIntent(intent: android.content.Intent) {
        val navigateTo = intent.getStringExtra("navigate_to") ?: return
        if (!navigateTo.startsWith("agentApproval/")) return
        // Route format: agentApproval/{questionId}/{cli}/{question}
        val parts = navigateTo.removePrefix("agentApproval/").split("/", limit = 3)
        if (parts.isEmpty() || parts[0].isEmpty()) return
        val questionId = parts[0]
        val cli = if (parts.size > 1) java.net.URLDecoder.decode(parts[1], "UTF-8") else ""
        val question = if (parts.size > 2) java.net.URLDecoder.decode(parts[2], "UTF-8") else ""
        // Store for TermFastApp to pick up and navigate
        pendingAgentApproval = Triple(questionId, cli, question)
        pendingAgentApprovalTick.value++
        android.util.Log.i("MainActivity", "handleAgentApprovalIntent: set pendingAgentApproval questionId=$questionId cli=$cli tick=${pendingAgentApprovalTick.value}")
        // Clear the extra so it doesn't re-trigger on config change
        intent.removeExtra("navigate_to")
    }

    companion object {
        /** Pending agent approval navigation (questionId, cli, question). Read by TermFastApp. */
        @Volatile
        var pendingAgentApproval: Triple<String, String, String>? = null
        /** Compose-observable counter that increments each time pendingAgentApproval is set.
         *  TermFastApp uses this as a LaunchedEffect key so it re-runs even when the
         *  composable is already in composition (e.g. app already running, notification tap). */
        val pendingAgentApprovalTick = androidx.compose.runtime.mutableIntStateOf(0)
    }
}
