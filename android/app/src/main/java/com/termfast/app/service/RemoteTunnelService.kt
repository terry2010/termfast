package com.termfast.app.service

import android.app.NotificationManager
import android.app.Service
import android.content.Context
import android.content.Intent
import android.os.IBinder
import androidx.core.content.ContextCompat
import com.termfast.app.data.RustEvent
import com.termfast.app.data.RustRepository
import com.termfast.app.ui.screen.TerminalSessionManager
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import org.json.JSONObject

/**
 * Foreground Service that keeps the tunnel process alive when the app is in background.
 *
 * The tunnel itself is managed by [TerminalSessionManager] (a singleton). This Service
 * only exists to elevate the process priority so the OS doesn't kill it, keeping the
 * WebSocket connection alive.
 *
 * It also listens for NOTIFY frames (agent_blocked / agent_resolved) and shows/clears
 * high-priority system notifications.
 */
class RemoteTunnelService : Service() {

    companion object {
        const val ACTION_STOP = "com.termfast.app.STOP_TUNNEL_SERVICE"

        // R3: 按 questionId 缓存 NOTIFY payload，AgentApprovalScreen 用 questionId 查缓存渲染 UI
        private val pendingPayloads = mutableMapOf<String, JSONObject>()

        fun cachePayload(questionId: String, payload: JSONObject) {
            synchronized(pendingPayloads) {
                pendingPayloads[questionId] = payload
                // 限制缓存大小，超过 50 条淘汰最老的（LRU 简化版）
                if (pendingPayloads.size > 50) {
                    pendingPayloads.keys.firstOrNull()?.let { pendingPayloads.remove(it) }
                }
            }
        }

        fun getPayload(questionId: String): JSONObject? {
            synchronized(pendingPayloads) {
                return pendingPayloads[questionId]
            }
        }

        fun removePayload(questionId: String) {
            synchronized(pendingPayloads) {
                pendingPayloads.remove(questionId)
            }
        }

        fun start(context: Context) {
            val intent = Intent(context, RemoteTunnelService::class.java)
            ContextCompat.startForegroundService(context, intent)
        }

        fun stop(context: Context) {
            val intent = Intent(context, RemoteTunnelService::class.java)
            intent.action = ACTION_STOP
            context.startService(intent)
        }
    }

    // 类级 scope，避免每次创建临时 scope 导致泄漏
    private val serviceScope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private var eventCollectorJob: Job? = null

    override fun onCreate() {
        super.onCreate()
        startForeground(
            NotificationHelper.NOTIF_TUNNEL_KEEPALIVE,
            NotificationHelper.buildTunnelNotification(this, "远程终端连接中")
        )
        startEventCollector()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_STOP) {
            if (!TerminalSessionManager.hasActiveTunnels()) {
                stopForeground(STOP_FOREGROUND_REMOVE)
                stopSelf()
            }
        }
        return START_STICKY
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onDestroy() {
        eventCollectorJob?.cancel()
        serviceScope.cancel()
        super.onDestroy()
    }

    private fun startEventCollector() {
        eventCollectorJob = serviceScope.launch {
            RustRepository.events.collect { event ->
                if (event is RustEvent.RemoteTerminalNotify) {
                    android.util.Log.d("termfast", "RemoteTunnelService received RemoteTerminalNotify: ${event.message}")
                    handleNotify(event)
                }
            }
        }
    }

    private fun handleNotify(event: RustEvent.RemoteTerminalNotify) {
        try {
            val json = JSONObject(event.message)
            // D6: list_changed 用 {"type":"list_changed"}，agent_* 用 event_type。
            // 本 Service 只处理 event_type。
            val eventType = json.optString("event_type")
            android.util.Log.d("termfast", "handleNotify: event_type=$eventType message=${event.message}")
            if (eventType == "agent_blocked") {
                val cli = json.optString("cli", "AI")
                val question = json.optString("question", "需要你的输入")
                val questionId = json.optString("question_id", "")
                // R3: 缓存完整 payload，添加 pairing_id 供 AgentApprovalScreen 查找 tunnel
                if (questionId.isNotEmpty()) {
                    json.put("pairing_id", event.pairing_id)
                    cachePayload(questionId, json)
                }
                // C1: agent_blocked 用 questionId.hashCode() 作 notification ID
                val notifId = if (questionId.isNotEmpty()) questionId.hashCode()
                    else NotificationHelper.NOTIF_TUNNEL_KEEPALIVE
                showAgentNotification(notifId, "$cli 需要你的输入: $question", questionId, cli, question)
            } else if (eventType == "agent_resolved") {
                val questionId = json.optString("question_id", "")
                // C1: 清除对应 questionId 的高优先级通知 + 清除缓存
                if (questionId.isNotEmpty()) {
                    val nm = getSystemService(NotificationManager::class.java)
                    nm.cancel(questionId.hashCode())
                    removePayload(questionId)
                }
            }
        } catch (e: Exception) {
            android.util.Log.e("termfast", "handleNotify exception: ${e.message}", e)
        }
    }

    // C1: agent_blocked 高优先级通知
    // 边界-1: 传 cli + question 给 buildAgentNotification，Intent extra 带最小渲染数据
    private fun showAgentNotification(
        notifId: Int,
        text: String,
        questionId: String,
        cli: String,
        question: String,
    ) {
        android.util.Log.d("termfast", "showAgentNotification: notifId=$notifId text=$text")
        try {
            val nm = getSystemService(NotificationManager::class.java)
            nm.notify(
                notifId,
                NotificationHelper.buildAgentNotification(this, notifId, text, questionId, cli, question)
            )
        } catch (e: Exception) {
            android.util.Log.e("termfast", "showAgentNotification failed", e)
        }
    }
}
