package com.termfast.app.service

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.os.Build
import androidx.core.app.NotificationCompat
import com.termfast.app.MainActivity
import com.termfast.app.R

object NotificationHelper {

    // v2: 换新 channel ID 以支持 IMPORTANCE_HIGH（Android channel importance 创建后不可修改）
    private const val CHANNEL_EVENTS = "termfast_events_v2"
    private const val CHANNEL_TRIGGERS = "termfast_triggers_v2"
    // D5: tunnel 保活 + agent blocked channels (C1)
    private const val CHANNEL_TUNNEL = "termfast_tunnel"
    // v3: 换新 channel ID 以支持 IMPORTANCE_HIGH + sound + vibration
    // (Android channel importance 创建后不可修改)
    private const val CHANNEL_AGENT = "termfast_agent_v3"

    const val NOTIF_CONNECT_SUCCESS = 1001
    const val NOTIF_DISCONNECT = 1002
    const val NOTIF_AUTH_FAIL = 1003
    const val NOTIF_TRIGGER_SUCCESS = 1004
    const val NOTIF_TRIGGER_FAIL = 1005
    const val NOTIF_IP_CHANGE = 1006
    // C1: tunnel 保活通知固定 ID
    const val NOTIF_TUNNEL_KEEPALIVE = 2001

    fun ensureChannels(context: Context) {
        val nm = context.getSystemService(NotificationManager::class.java)
        if (nm.getNotificationChannel(CHANNEL_EVENTS) == null) {
            val channel = NotificationChannel(
                CHANNEL_EVENTS,
                "事件通知",
                NotificationManager.IMPORTANCE_HIGH
            ).apply {
                description = "连接状态、IP 变化等事件通知"
                enableVibration(true)
                enableLights(true)
            }
            nm.createNotificationChannel(channel)
        }
        if (nm.getNotificationChannel(CHANNEL_TRIGGERS) == null) {
            val channel = NotificationChannel(
                CHANNEL_TRIGGERS,
                "触发器通知",
                NotificationManager.IMPORTANCE_HIGH
            ).apply {
                description = "触发器执行成功/失败通知"
                enableVibration(true)
                enableLights(true)
            }
            nm.createNotificationChannel(channel)
        }
        // C1: tunnel 保活 channel（低优先级，折叠，不响铃）
        if (nm.getNotificationChannel(CHANNEL_TUNNEL) == null) {
            val channel = NotificationChannel(
                CHANNEL_TUNNEL,
                "远程终端保活",
                NotificationManager.IMPORTANCE_MIN
            ).apply {
                description = "远程终端 tunnel 连接保活通知"
                setShowBadge(false)
            }
            nm.createNotificationChannel(channel)
        }
        // C1: agent blocked channel（高优先级，弹出，有声音）
        if (nm.getNotificationChannel(CHANNEL_AGENT) == null) {
            val channel = NotificationChannel(
                CHANNEL_AGENT,
                "AI 需要输入",
                NotificationManager.IMPORTANCE_HIGH
            ).apply {
                description = "AI CLI 需要你的输入时的高优先级通知"
                enableVibration(true)
                enableLights(true)
                lockscreenVisibility = android.app.Notification.VISIBILITY_PUBLIC
                // Allow bypass of Do Not Disturb
                setBypassDnd(true)
            }
            nm.createNotificationChannel(channel)
        }
    }

    // C1: tunnel 保活通知（固定 ID 2001，低优先级）
    // P10: 无 questionId 参数
    fun buildTunnelNotification(
        context: Context,
        text: String,
    ): android.app.Notification {
        ensureChannels(context)
        val intent = Intent(context, MainActivity::class.java)
        val pi = PendingIntent.getActivity(
            context, 0, intent,
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        )
        return NotificationCompat.Builder(context, CHANNEL_TUNNEL)
            .setContentTitle("TermFast")
            .setContentText(text)
            .setSmallIcon(R.drawable.ic_tile_vpn)
            .setContentIntent(pi)
            .setOngoing(true)
            .setVisibility(NotificationCompat.VISIBILITY_PRIVATE)
            .setShowWhen(false)
            .build()
    }

    // C1: agent_blocked 高优先级通知（用 questionId.hashCode() 作 ID，支持多终端堆叠）
    // 边界-1: Intent extra 带最小渲染数据（cli + question），进程被杀缓存丢失时降级
    fun buildAgentNotification(
        context: Context,
        notifId: Int,
        text: String,
        questionId: String,
        cli: String,
        question: String,
    ): android.app.Notification {
        ensureChannels(context)
        // Encode cli + question into the route for fallback rendering (边界-1)
        val encodedCli = java.net.URLEncoder.encode(cli, "UTF-8")
        val encodedQuestion = java.net.URLEncoder.encode(question, "UTF-8")
        val intent = Intent(context, MainActivity::class.java).apply {
            putExtra("navigate_to", "agentApproval/$questionId/$encodedCli/$encodedQuestion")
            flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP
        }
        val pi = PendingIntent.getActivity(
            context, notifId, intent,
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        )
        return NotificationCompat.Builder(context, CHANNEL_AGENT)
            .setContentTitle("$cli 需要你的输入")
            .setContentText(question)
            .setSmallIcon(R.drawable.ic_agent_blocked)
            .setContentIntent(pi)
            .setFullScreenIntent(pi, true)
            // 不设 ongoing — MIUI 不会对 ongoing 通知弹 heads-up 横幅。
            // agent_resolved 到达后 nm.cancel() 主动清除。
            .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
            .setPriority(NotificationCompat.PRIORITY_MAX)
            .setCategory(NotificationCompat.CATEGORY_CALL)
            .setStyle(NotificationCompat.BigTextStyle().bigText(question))
            // Prevent auto-grouping which suppresses heads-up on MIUI
            .setGroup(null)
            // Heads-up notification defaults
            .setDefaults(NotificationCompat.DEFAULT_ALL)
            .setAutoCancel(true)
            .build()
    }

    fun sendEventNotification(
        context: Context,
        id: Int,
        title: String,
        text: String,
    ) {
        ensureChannels(context)
        val intent = Intent(context, MainActivity::class.java)
        val pi = PendingIntent.getActivity(
            context, id, intent,
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        )
        val notification = NotificationCompat.Builder(context, CHANNEL_EVENTS)
            .setContentTitle(title)
            .setContentText(text)
            .setSmallIcon(android.R.drawable.ic_dialog_info)
            .setContentIntent(pi)
            .setAutoCancel(true)
            .setVisibility(NotificationCompat.VISIBILITY_PRIVATE)
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .setCategory(NotificationCompat.CATEGORY_MESSAGE)
            .build()
        val nm = context.getSystemService(NotificationManager::class.java)
        nm.notify(id, notification)
    }

    fun sendTriggerNotification(
        context: Context,
        success: Boolean,
        triggerName: String,
        detail: String,
    ) {
        ensureChannels(context)
        val id = if (success) NOTIF_TRIGGER_SUCCESS else NOTIF_TRIGGER_FAIL
        val title = if (success) "触发器成功: $triggerName" else "触发器失败: $triggerName"
        val intent = Intent(context, MainActivity::class.java)
        val pi = PendingIntent.getActivity(
            context, id, intent,
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        )
        val notification = NotificationCompat.Builder(context, CHANNEL_TRIGGERS)
            .setContentTitle(title)
            .setContentText(detail)
            .setSmallIcon(android.R.drawable.ic_dialog_info)
            .setContentIntent(pi)
            .setAutoCancel(true)
            .setVisibility(NotificationCompat.VISIBILITY_PRIVATE)
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .setCategory(NotificationCompat.CATEGORY_MESSAGE)
            .build()
        val nm = context.getSystemService(NotificationManager::class.java)
        nm.notify(id, notification)
    }
}
