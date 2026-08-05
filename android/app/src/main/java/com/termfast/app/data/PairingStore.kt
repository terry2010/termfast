package com.termfast.app.data

import android.content.Context
import android.content.SharedPreferences

object PairingStore {
    private const val PREFS_NAME = "pairing"
    private const val KEY_TOKEN = "token"
    private const val KEY_PAIRING_JWT = "pairing_jwt"
    private const val KEY_PAIRING_ID = "pairing_id"
    private const val KEY_PAIRING_KEY = "pairing_key"
    private const val KEY_RELAY_URL = "relay_url"

    private var ctx: Context? = null

    fun init(context: Context) {
        ctx = context.applicationContext
    }

    private fun prefs(): SharedPreferences {
        val c = ctx ?: throw IllegalStateException("PairingStore not initialized")
        return c.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
    }

    fun saveToken(token: String) {
        prefs().edit().putString(KEY_TOKEN, token).apply()
    }

    fun getToken(): String? = prefs().getString(KEY_TOKEN, null)

    fun clearToken() {
        prefs().edit().remove(KEY_TOKEN).apply()
    }

    fun savePairingJwt(jwt: String) {
        prefs().edit().putString(KEY_PAIRING_JWT, jwt).apply()
    }

    fun getPairingJwt(): String? = prefs().getString(KEY_PAIRING_JWT, null)

    fun clearPairingJwt() {
        prefs().edit().remove(KEY_PAIRING_JWT).apply()
    }

    // --- Remote terminal tunnel credentials ---

    /** Save pairing ID for remote terminal tunnel. */
    fun savePairingId(pairingId: String) {
        prefs().edit().putString(KEY_PAIRING_ID, pairingId).apply()
    }

    fun getPairingId(): String? = prefs().getString(KEY_PAIRING_ID, null)

    /** Save pairing key (hex-encoded 32-byte key) for frame crypto. */
    fun savePairingKey(pairingKey: String) {
        prefs().edit().putString(KEY_PAIRING_KEY, pairingKey).apply()
    }

    fun getPairingKey(): String? = prefs().getString(KEY_PAIRING_KEY, null)

    /** Save relay URL for WebSocket tunnel. */
    fun saveRelayUrl(relayUrl: String) {
        prefs().edit().putString(KEY_RELAY_URL, relayUrl).apply()
    }

    fun getRelayUrl(): String? = prefs().getString(KEY_RELAY_URL, null)

    /** Save all remote tunnel credentials at once. */
    fun saveRemoteTunnelConfig(pairingId: String, pairingKey: String, relayUrl: String, jwt: String) {
        prefs().edit()
            .putString(KEY_PAIRING_ID, pairingId)
            .putString(KEY_PAIRING_KEY, pairingKey)
            .putString(KEY_RELAY_URL, relayUrl)
            .putString(KEY_PAIRING_JWT, jwt)
            .apply()
    }

    /** Check if remote tunnel config is complete (all 4 fields present). */
    fun hasRemoteTunnelConfig(): Boolean {
        return getPairingId() != null &&
            getPairingKey() != null &&
            getRelayUrl() != null &&
            getPairingJwt() != null
    }

    /** Clear all remote tunnel credentials. */
    fun clearRemoteTunnelConfig() {
        prefs().edit()
            .remove(KEY_PAIRING_ID)
            .remove(KEY_PAIRING_KEY)
            .remove(KEY_RELAY_URL)
            .remove(KEY_PAIRING_JWT)
            .apply()
    }
}
