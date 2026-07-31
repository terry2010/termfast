package com.termfast.app.service

import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.graphics.drawable.Icon
import android.net.VpnService
import android.os.Build
import android.os.IBinder
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService
import com.termfast.app.MainActivity
import com.termfast.app.R
import com.termfast.app.data.SettingsRepository

class SshVpnTileService : TileService() {

    override fun onStartListening() {
        super.onStartListening()
        updateTile()
    }

    override fun onClick() {
        super.onClick()
        val context = this
        if (SshVpnService.isRunning(context)) {
            SshVpnService.stop(context)
        } else {
            val serverId = getLastServerId(context) ?: return
            val prepare = VpnService.prepare(context)
            if (prepare != null) {
                val intent = Intent(context, MainActivity::class.java).apply {
                    flags = Intent.FLAG_ACTIVITY_NEW_TASK
                    putExtra("start_vpn", true)
                    putExtra("server_id", serverId)
                }
                startActivityAndCollapse(intent)
            } else {
                val settings = SettingsRepository(context).load()
                SshVpnService.start(context, serverId, settings)
            }
        }
        updateTile()
    }

    private fun updateTile() {
        val tile = qsTile ?: return
        val running = SshVpnService.isRunning(this)
        tile.state = if (running) Tile.STATE_ACTIVE else Tile.STATE_INACTIVE
        tile.label = "TermFast VPN"
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            tile.subtitle = if (running) "运行中" else "已停止"
        }
        tile.contentDescription = "TermFast VPN 快捷开关"
        tile.icon = Icon.createWithResource(this, R.drawable.ic_tile_vpn)
        tile.updateTile()
    }

    override fun onBind(intent: Intent?): IBinder? {
        requestListeningState(this, ComponentName(this, SshVpnTileService::class.java))
        return super.onBind(intent)
    }

    companion object {
        private const val PREFS_TILE = "termfast_tile"
        private const val KEY_LAST_SERVER = "last_server_id"

        fun setLastServerId(context: Context, serverId: String?) {
            context.getSharedPreferences(PREFS_TILE, Context.MODE_PRIVATE)
                .edit().putString(KEY_LAST_SERVER, serverId).commit()
        }

        private fun getLastServerId(context: Context): String? {
            return context.getSharedPreferences(PREFS_TILE, Context.MODE_PRIVATE)
                .getString(KEY_LAST_SERVER, null)
        }
    }
}
