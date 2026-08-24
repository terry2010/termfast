package com.termfast.app.data

import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.Context
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat

/**
 * Helper to display system notifications from NOTIFY frames.
 */
object NotificationHelper {
    // D5: channel id 从 termfast_events 改名为 termfast_data_events（避免与 service/ 的重名）
    // v2: 改为 IMPORTANCE_HIGH 以支持 heads-up 横幅强提醒。
    //     Android channel importance 创建后不可修改，换新 ID 确保升级生效。
    private const val CHANNEL_ID = "termfast_data_events_v2"
    private const val CHANNEL_NAME = "TermFast Events"

    fun createChannel(context: Context) {
        val channel = NotificationChannel(
            CHANNEL_ID,
            CHANNEL_NAME,
            NotificationManager.IMPORTANCE_HIGH
        ).apply {
            description = "Notifications from desktop terminal events"
            enableVibration(true)
            enableLights(true)
        }
        val manager = context.getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(channel)
    }

    fun showNotification(
        context: Context,
        title: String,
        body: String,
        notificationId: Int,
    ) {
        val builder = NotificationCompat.Builder(context, CHANNEL_ID)
            .setSmallIcon(android.R.drawable.ic_dialog_info)
            .setContentTitle(title)
            .setContentText(body)
            .setStyle(NotificationCompat.BigTextStyle().bigText(body))
            .setAutoCancel(true)
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .setCategory(NotificationCompat.CATEGORY_MESSAGE)
        try {
            NotificationManagerCompat.from(context).notify(notificationId, builder.build())
        } catch (e: SecurityException) {
            // POST_NOTIFICATIONS permission not granted — ignore
        }
    }
}
