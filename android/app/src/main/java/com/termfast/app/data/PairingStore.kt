package com.termfast.app.data

import android.content.Context
import android.content.SharedPreferences
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import kotlinx.serialization.encodeToString

@Serializable
data class RemoteTunnelConfig(
    val pairingId: String,
    val pairingKey: String,
    val relayUrl: String,
    val pairingJwt: String,
    val desktopName: String,
    val desktopDeviceId: String,
)

object PairingStore {
    private const val PREFS_NAME = "pairing"
    private const val KEY_TOKEN = "token"
    private const val KEY_PAIRINGS = "pairings_json"

    private var ctx: Context? = null
    private val json = Json { ignoreUnknownKeys = true }

    fun init(context: Context) {
        ctx = context.applicationContext
    }

    private fun prefs(): SharedPreferences {
        val c = ctx ?: throw IllegalStateException("PairingStore not initialized")
        return c.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
    }

    // --- User account token (not per-pairing) ---

    fun saveToken(token: String) {
        prefs().edit().putString(KEY_TOKEN, token).apply()
    }

    fun getToken(): String? = prefs().getString(KEY_TOKEN, null)

    fun clearToken() {
        prefs().edit().remove(KEY_TOKEN).apply()
    }

    // --- Multi-pairing storage ---

    private fun readMap(): MutableMap<String, RemoteTunnelConfig> {
        val raw = prefs().getString(KEY_PAIRINGS, null) ?: return mutableMapOf()
        return try {
            val list = json.decodeFromString<List<RemoteTunnelConfig>>(raw)
            list.associateBy { it.pairingId }.toMutableMap()
        } catch (_: Exception) {
            mutableMapOf()
        }
    }

    private fun writeMap(map: Map<String, RemoteTunnelConfig>) {
        prefs().edit().putString(KEY_PAIRINGS, json.encodeToString(map.values.toList())).apply()
    }

    /** Save a pairing. Overwrites any existing pairing with the same desktopDeviceId. */
    fun savePairing(config: RemoteTunnelConfig) {
        val map = readMap()
        // Dedup by desktopDeviceId: remove old pairing for same desktop
        map.values.removeAll { it.desktopDeviceId == config.desktopDeviceId && it.pairingId != config.pairingId }
        map[config.pairingId] = config
        writeMap(map)
    }

    fun getAllPairings(): List<RemoteTunnelConfig> = readMap().values.toList()

    fun getPairing(pairingId: String): RemoteTunnelConfig? = readMap()[pairingId]

    /** Find a mobile pairing by desktop device ID (used by desktop-pair QR scan flow). */
    fun getPairingByDeviceId(desktopDeviceId: String): RemoteTunnelConfig? =
        readMap().values.find { it.desktopDeviceId == desktopDeviceId }

    fun removePairing(pairingId: String) {
        val map = readMap()
        map.remove(pairingId)
        writeMap(map)
    }

    /** Clear all stored data (token + pairings). Used on logout. */
    fun clearAll() {
        prefs().edit().clear().apply()
    }
}
