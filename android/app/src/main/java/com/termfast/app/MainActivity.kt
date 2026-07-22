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
        val dataDir = filesDir.absolutePath
        RustRepository.init(dataDir)
        CloudSyncManager.appContext = applicationContext
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
        }
    }

    override fun onNewIntent(intent: android.content.Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        handleStartVpnIntent(intent)
        handleOAuthDeepLink(intent)
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
}
