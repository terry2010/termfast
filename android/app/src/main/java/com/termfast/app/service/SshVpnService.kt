package com.termfast.app.service

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.VpnService
import android.os.Build
import android.os.ParcelFileDescriptor
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import com.termfast.app.MainActivity
import com.termfast.app.RustBridge
import com.termfast.app.data.AppSettings

class SshVpnService : VpnService() {

    enum class VpnState { STOPPED, STARTING, RUNNING, FAILED }

    companion object {
        const val EXTRA_SERVER_ID = "server_id"
        const val EXTRA_MTU = "mtu"
        const val EXTRA_SOCKS5_PORT = "socks5_port"
        private const val CHANNEL_ID = "termfast_vpn"
        private const val NOTIFICATION_ID = 1
        private const val TAG = "SshVpnService"

        @Volatile
        private var state = VpnState.STOPPED

        @Volatile
        var activeServerId: String = ""
            private set

        @Volatile
        private var previousActiveServerId: String = ""

        @Volatile
        var lastError: String? = null
            private set

        @Volatile
        var failedServerId: String? = null
            private set

        fun isRunning(context: Context): Boolean = state == VpnState.RUNNING
        fun isStarting(context: Context): Boolean = state == VpnState.STARTING
        fun isFailed(context: Context): Boolean = state == VpnState.FAILED
        fun isFailedFor(context: Context, serverId: String): Boolean =
            state == VpnState.FAILED && failedServerId == serverId
        fun isActive(context: Context): Boolean = state != VpnState.STOPPED

        fun setFailed(serverId: String, error: String) {
            failedServerId = serverId
            lastError = error
            state = VpnState.FAILED
        }

        fun clearError() {
            lastError = null
            failedServerId = null
        }

        fun start(context: Context, serverId: String, settings: AppSettings = AppSettings(), socks5Port: Int = 1080) {
            val intent = Intent(context, SshVpnService::class.java).apply {
                putExtra(EXTRA_SERVER_ID, serverId)
                putExtra(EXTRA_MTU, settings.vpnMtu)
                putExtra(EXTRA_SOCKS5_PORT, socks5Port)
                putExtra("ipv6", settings.ipv6Enabled)
                putExtra("route_ula", settings.routeUla)
                putExtra("dns", settings.dnsStrategy)
                putExtra("kill_switch", settings.killSwitchEnabled)
                putExtra("per_app_mode", settings.perAppMode)
                putExtra("per_app_packages", settings.perAppPackages.toTypedArray())
            }
            context.startForegroundService(intent)
        }

        fun stop(context: Context) {
            // Send a stop command to the service so it can clean up synchronously
            val intent = Intent(context, SshVpnService::class.java).apply {
                putExtra("action", "stop")
            }
            context.startService(intent)
        }
    }

    private var tunFd: ParcelFileDescriptor? = null
    private var tunFdRaw: Int = -1
    private var serverId: String = ""

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent == null) {
            stopSelf()
            return START_NOT_STICKY
        }

        // Handle stop action
        if (intent.getStringExtra("action") == "stop") {
            Log.i(TAG, "Stop action received")
            state = VpnState.STOPPED
            // Save the current server ID before clearing, so that if a start
            // action follows immediately, startVpnInternal can use
            // previousActiveServerId to tear down the old connection.
            previousActiveServerId = activeServerId
            activeServerId = ""
            Thread {
                try {
                    RustBridge.nativeStopVpn(serverId)
                    RustBridge.nativeStopProxy(serverId)
                    RustBridge.nativeDisconnectServer(serverId)
                    RustBridge.nativeClearProtectCallback()
                } catch (e: Exception) {
                    Log.e(TAG, "Error stopping VPN", e)
                }
                closeTunFd()
                stopForeground(STOP_FOREGROUND_REMOVE)
                stopSelf()
            }.start()
            return START_NOT_STICKY
        }

        serverId = intent.getStringExtra(EXTRA_SERVER_ID) ?: ""
        // Save the currently active server (if any) before switching, so
        // startVpnInternal can tear down its SSH/proxy/VPN state.
        previousActiveServerId = activeServerId
        activeServerId = serverId
        Log.i(TAG, "Starting VPN for server=$serverId, previous=$previousActiveServerId")
        val mtu = intent.getIntExtra(EXTRA_MTU, 1400)
        val socks5Port = intent.getIntExtra(EXTRA_SOCKS5_PORT, 1080)
        val ipv6 = intent.getBooleanExtra("ipv6", true)
        val routeUla = intent.getBooleanExtra("route_ula", false)
        val dns = "none"  // Virtual DNS: tun2proxy returns fake IPs, resolves domains
                          // via SOCKS5 hostname resolution (SSH direct-tcpip) on remote.
                          // over-tcp doesn't work: remote can't reach 8.8.8.8:53 reliably.
        val killSwitch = intent.getBooleanExtra("kill_switch", false)
        val perAppMode = intent.getStringExtra("per_app_mode") ?: "blacklist"
        val perAppPackages = intent.getStringArrayExtra("per_app_packages") ?: emptyArray()

        // Mark as starting immediately so UI shows "连接中" during async startup
        state = VpnState.STARTING
        clearError()

        createNotificationChannel()
        // Use ServiceCompat for backward compatibility with minSdk=26
        ServiceCompat.startForeground(
            this,
            NOTIFICATION_ID,
            buildNotification("正在启动 VPN..."),
            ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE,
        )

        // Set up socket protect callback so SSH connections bypass the TUN
        RustBridge.nativeSetProtectCallback(this)

        // Run the full VPN bring-up sequence in a background thread to avoid ANR
        Thread {
            try {
                startVpnInternal(mtu, socks5Port, ipv6, routeUla, dns, killSwitch, perAppMode, perAppPackages)
            } catch (e: Exception) {
                Log.e(TAG, "Failed to start VPN", e)
                state = VpnState.STOPPED
                if (killSwitch) {
                    updateNotification("VPN 异常，Kill switch 已阻止流量")
                } else {
                    stopSelf()
                }
            }
        }.start()

        return START_STICKY
    }

    private fun startVpnInternal(
        mtu: Int,
        socks5Port: Int,
        ipv6: Boolean,
        routeUla: Boolean,
        dns: String,
        killSwitch: Boolean,
        perAppMode: String,
        perAppPackages: Array<String>
    ) {
        // Ensure SSH is connected and proxy is running before bringing up the TUN
        if (serverId.isEmpty()) {
            Log.e(TAG, "No server id provided")
            state = VpnState.STOPPED
            stopSelf()
            return
        }

        // Tear down any previous VPN/proxy/SSH state before connecting.
        // This handles the case where the user switches directly from one
        // server to another without explicitly stopping the first VPN.
        // We do a full teardown here (not relying on the async stop action)
        // to ensure the old state is fully cleared before connecting.
        val previousServerId = previousActiveServerId
        if (previousServerId.isNotEmpty() && previousServerId != serverId) {
            // Stop tun2proxy first
            RustBridge.nativeStopVpn(previousServerId)
            // Close the old TUN fd — this removes Android's VPN routing so
            // DNS resolution in nativeConnectServer goes through the underlying
            // network instead of the dead TUN interface.
            closeTunFd()
            // Stop proxy and disconnect SSH for the previous server
            RustBridge.nativeStopProxy(previousServerId)
            RustBridge.nativeDisconnectServer(previousServerId)
        } else {
            // Same server or no previous — just stop tun2proxy and close TUN
            RustBridge.nativeStopVpn(serverId)
            closeTunFd()
        }
        // Disconnect the new server's SSH (was made without protect), then reconnect
        RustBridge.nativeDisconnectServer(serverId)

        if (!RustBridge.nativeConnectServer(serverId)) {
            Log.e(TAG, "SSH connection failed")
            val detail = RustBridge.nativeGetLastError()
            val msg = if (detail.isNotBlank()) detail else "SSH 连接失败"
            setFailed(serverId, msg)
            updateNotification(msg)
            stopSelf()
            return
        }

        if (!RustBridge.nativeStartProxy(serverId, socks5Port, 0, 0)) {
            Log.e(TAG, "Proxy start failed")
            val detail = RustBridge.nativeGetLastError()
            val msg = if (detail.isNotBlank()) detail else "代理启动失败"
            setFailed(serverId, msg)
            updateNotification(msg)
            stopSelf()
            return
        }

        try {
            val builder = Builder()
                .setSession("TermFast VPN")
                .setMtu(mtu)
                .addAddress("10.0.0.2", 32)
                .addRoute("0.0.0.0", 0)
                .setUnderlyingNetworks(null)

            // DNS — with Virtual DNS (dns=none), tun2proxy intercepts all DNS
            // queries and returns fake IPs. The addDnsServer address is only
            // used by the system for DoT/DoH fallback. Use 223.5.5.5 (Alibaba
            // DNS) which is reachable from both China and overseas servers,
            // avoiding Google DoT connection failures on China-based servers.
            builder.addDnsServer("223.5.5.5")
            if (ipv6) {
                builder.addDnsServer("2400:3200::1")
            }

            if (ipv6) {
                builder.addRoute("2000::", 3)
                if (routeUla) {
                    builder.addRoute("fc00::", 7)
                }
            }

            // Per-app proxy
            if (perAppMode == "whitelist") {
                perAppPackages.forEach { builder.addAllowedApplication(it) }
            } else {
                perAppPackages.forEach { builder.addDisallowedApplication(it) }
            }

            tunFd = builder.establish()
            if (tunFd == null) {
                Log.e(TAG, "VpnService.establish() returned null — permission revoked?")
                state = VpnState.STOPPED
                stopSelf()
                return
            }
            val fdInt = tunFd!!.detachFd()
            tunFd = null  // detached, fdInt is now owned by Rust/tun2proxy
            tunFdRaw = fdInt  // keep raw fd for later close
            val ok = RustBridge.nativeStartVpn(serverId, fdInt, mtu, socks5Port, dns, ipv6)
            if (!ok) {
                Log.e(TAG, "nativeStartVpn failed")
                setFailed(serverId, "VPN 启动失败")
                updateNotification("VPN 启动失败")
                stopSelf()
                return
            }
            state = VpnState.RUNNING
            clearError()
            updateNotification("VPN 运行中")
        } catch (e: Exception) {
            Log.e(TAG, "Failed to start VPN", e)
            setFailed(serverId, "VPN 启动异常: ${e.message}")
            updateNotification("VPN 启动异常")
            stopSelf()
        }
    }

    private fun establishEmptyTun(mtu: Int, killSwitch: Boolean) {
        if (!killSwitch) return
        try {
            val builder = Builder()
                .setSession("TermFast VPN")
                .setMtu(mtu)
                .addAddress("10.0.0.2", 32)
                .addRoute("0.0.0.0", 0)
            tunFd = builder.establish()
            if (tunFd != null) {
                updateNotification("Kill switch 已阻止流量")
            }
        } catch (e: Exception) {
            Log.e(TAG, "Failed to establish empty TUN", e)
        }
    }

    private fun closeTunFd() {
        // Use ParcelFileDescriptor.adoptFd to properly close the raw fd
        if (tunFdRaw >= 0) {
            try {
                val pfd = ParcelFileDescriptor.adoptFd(tunFdRaw)
                pfd.close()
                Log.i(TAG, "TUN fd closed successfully")
            } catch (e: Exception) {
                Log.e(TAG, "Failed to close tun fd via PFD", e)
            }
            tunFdRaw = -1
        }
        tunFd?.close()
        tunFd = null
    }

    override fun onRevoke() {
        Log.w(TAG, "VPN revoked by system")
        state = VpnState.STOPPED
        Thread {
            RustBridge.nativeStopVpn(serverId)
            RustBridge.nativeClearProtectCallback()
            closeTunFd()
            stopSelf()
        }.start()
    }

    override fun onDestroy() {
        super.onDestroy()
        Log.i(TAG, "onDestroy called")
        // Don't overwrite FAILED state — the UI needs it to show the error.
        // Only reset to STOPPED if we were RUNNING or STARTING.
        if (state == VpnState.RUNNING || state == VpnState.STARTING) {
            state = VpnState.STOPPED
        }
        Thread {
            RustBridge.nativeStopVpn(serverId)
            RustBridge.nativeClearProtectCallback()
            closeTunFd()
        }.start()
    }

    private fun createNotificationChannel() {
        val nm = getSystemService(NotificationManager::class.java)
        if (nm.getNotificationChannel(CHANNEL_ID) == null) {
            val channel = NotificationChannel(
                CHANNEL_ID,
                "TermFast VPN",
                NotificationManager.IMPORTANCE_LOW
            ).apply {
                description = "VPN 前台服务通知"
            }
            nm.createNotificationChannel(channel)
        }
    }

    private fun buildNotification(text: String): Notification {
        val intent = Intent(this, MainActivity::class.java)
        val pi = PendingIntent.getActivity(
            this, 0, intent,
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        )
        return NotificationCompat.Builder(this, CHANNEL_ID)
            .setContentTitle("TermFast")
            .setContentText(text)
            .setSmallIcon(android.R.drawable.ic_menu_info_details)
            .setContentIntent(pi)
            .setOngoing(true)
            .build()
    }

    private fun updateNotification(text: String) {
        val nm = getSystemService(NotificationManager::class.java)
        nm.notify(NOTIFICATION_ID, buildNotification(text))
    }
}
