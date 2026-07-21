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

    private const val CHANNEL_EVENTS = "termfast_events"
    private const val CHANNEL_TRIGGERS = "termfast_triggers"

    const val NOTIF_CONNECT_SUCCESS = 1001
    const val NOTIF_DISCONNECT = 1002
    const val NOTIF_AUTH_FAIL = 1003
    const val NOTIF_TRIGGER_SUCCESS = 1004
    const val NOTIF_TRIGGER_FAIL = 1005
    const val NOTIF_IP_CHANGE = 1006

    fun ensureChannels(context: Context) {
        val nm = context.getSystemService(NotificationManager::class.java)
        if (nm.getNotificationChannel(CHANNEL_EVENTS) == null) {
            val channel = NotificationChannel(
                CHANNEL_EVENTS,
                "事件通知",
                NotificationManager.IMPORTANCE_DEFAULT
            ).apply {
                description = "连接状态、IP 变化等事件通知"
            }
            nm.createNotificationChannel(channel)
        }
        if (nm.getNotificationChannel(CHANNEL_TRIGGERS) == null) {
            val channel = NotificationChannel(
                CHANNEL_TRIGGERS,
                "触发器通知",
                NotificationManager.IMPORTANCE_DEFAULT
            ).apply {
                description = "触发器执行成功/失败通知"
            }
            nm.createNotificationChannel(channel)
        }
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
            .build()
        val nm = context.getSystemService(NotificationManager::class.java)
        nm.notify(id, notification)
    }
}
