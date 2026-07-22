package com.termfast.app.data

import android.util.Log
import com.termfast.app.RustBridge
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonPrimitive

/**
 * Cloud sync manager — wraps RustBridge JNI calls for cloud sync operations.
 *
 * OAuth flow on Android:
 * 1. User taps "Connect Dropbox/Baidu" → call [authUrl] to get the OAuth URL
 * 2. Open the URL in the browser → user authorizes
 * 3. Provider redirects to cloud-sync-callback.php → script redirects to
 *    termfast://oauth/callback?code=xxx&state=xxx
 * 4. Android catches the deep link → MainActivity passes `code` to [exchangeCode]
 * 5. [exchangeCode] returns a token JSON → [saveToken] stores it
 *
 * Sync flow:
 * - [upload]: encrypts config with master password, uploads to cloud
 *   (with conflict detection — returns conflict JSON if cloud changed)
 * - [download]: downloads + decrypts + applies config
 *   (with rollback detection — returns rollback JSON if cloud is older)
 */
object CloudSyncManager {
    private const val TAG = "CloudSyncManager"
    private const val PREFS_NAME = "cloud_sync_cache"
    private val json = Json { ignoreUnknownKeys = true }

    /** Cloud provider identifiers. */
    object Provider {
        const val DROPBOX = "dropbox"
        const val BAIDU = "baidu"
    }

    /** OAuth callback events emitted when a deep link is received. */
    sealed class OAuthEvent {
        /** OAuth code exchange succeeded, token saved. */
        data class Success(val provider: String) : OAuthEvent()
        /** OAuth code exchange failed. */
        data class Error(val message: String) : OAuthEvent()
        /** OAuth flow cancelled by user (error=access_denied in callback). */
        object Cancelled : OAuthEvent()
    }

    private val _oauthEvents = MutableSharedFlow<OAuthEvent>(extraBufferCapacity = 4)
    val oauthEvents: SharedFlow<OAuthEvent> = _oauthEvents.asSharedFlow()

    /**
     * Handle the OAuth deep link callback (termfast://oauth/callback?code=...).
     * Called from MainActivity.onNewIntent / onCreate when the intent matches.
     * Exchanges the code for a token and saves it, then emits an [OAuthEvent].
     */
    suspend fun handleDeepLink(uri: android.net.Uri) {
        val code = uri.getQueryParameter("code")
        val error = uri.getQueryParameter("error")
        if (error != null) {
            _oauthEvents.tryEmit(OAuthEvent.Cancelled)
            return
        }
        if (code.isNullOrBlank()) {
            _oauthEvents.tryEmit(OAuthEvent.Error("回调缺少 code 参数"))
            return
        }
        val token = exchangeCode(code)
        if (token == null) {
            _oauthEvents.tryEmit(OAuthEvent.Error("换取 token 失败"))
            return
        }
        val ok = saveToken(token)
        if (ok) {
            _oauthEvents.tryEmit(OAuthEvent.Success(token.provider))
        } else {
            _oauthEvents.tryEmit(OAuthEvent.Error("保存 token 失败"))
        }
    }

    /** Result of auth URL generation. */
    @Serializable
    data class AuthUrlResult(
        val auth_url: String,
        val provider: String,
    )

    /** Result of token exchange. */
    @Serializable
    data class TokenResult(
        val access_token: String,
        val refresh_token: String? = null,
        val expires_at: Long? = null,
        val token_type: String = "bearer",
        val provider: String,
    )

    /** Cloud sync status. */
    @Serializable
    data class SyncStatus(
        val authenticated: Boolean = false,
        val has_remote: Boolean = false,
        val remote_size: Long? = null,
        val remote_modified: String? = null,
        val last_synced: String? = null,
        val last_device: String? = null,
    )

    /** Upload/download response. */
    @Serializable
    data class SyncResponse(
        val ok: Boolean = false,
        val conflict: Boolean = false,
        val reason: String? = null,
        val message: String? = null,
        val device_name: String? = null,
        val updated_at: String? = null,
        val size: Long? = null,
        val cloud_updated_at: String? = null,
        val last_updated_at: String? = null,
        val local_updated_at: String? = null,
    )

    // === SECTION cloud_sync_manager_1 END ===

    /** Step 1: Get the OAuth authorization URL. */
    fun authUrl(provider: String): AuthUrlResult? {
        val raw = RustBridge.nativeCloudSyncAuthUrl(provider)
        if (raw.isBlank()) return null
        return runCatching { json.decodeFromString(AuthUrlResult.serializer(), raw) }
            .onFailure { Log.e(TAG, "authUrl parse failed: $it") }
            .getOrNull()
    }

    /** Step 4: Exchange the OAuth code for a token (called from deep link callback). */
    fun exchangeCode(code: String): TokenResult? {
        val raw = RustBridge.nativeCloudSyncExchangeCode(code)
        if (raw.isBlank()) return null
        return runCatching { json.decodeFromString(TokenResult.serializer(), raw) }
            .onFailure { Log.e(TAG, "exchangeCode parse failed: $it") }
            .getOrNull()
    }

    /** Step 5: Save the token to the token store. */
    fun saveToken(token: TokenResult): Boolean {
        val tokenJson = json.encodeToString(TokenResult.serializer(), token)
        return RustBridge.nativeCloudSyncSaveToken(tokenJson)
    }

    /** Check if a provider is authenticated. */
    fun loadToken(provider: String): SyncStatus {
        val raw = RustBridge.nativeCloudSyncLoadToken(provider)
        return runCatching { json.decodeFromString(SyncStatus.serializer(), raw) }
            .onFailure { Log.e(TAG, "loadToken parse failed: $it") }
            .getOrDefault(SyncStatus())
    }

    /** Get full sync status (authenticated + remote file info + last sync). */
    fun status(provider: String): SyncStatus {
        val raw = RustBridge.nativeCloudSyncStatus(provider)
        val rustStatus = runCatching { json.decodeFromString(SyncStatus.serializer(), raw) }
            .onFailure { Log.e(TAG, "status parse failed: $it") }
            .getOrDefault(SyncStatus())
        // Rust 端 sync_state 用主密码加密，status 无密码时解密失败，
        // last_synced 为 null。用 SharedPreferences 缓存补充。
        val (cachedSynced, cachedDevice) = readCache(provider)
        return rustStatus.copy(
            last_synced = rustStatus.last_synced ?: cachedSynced,
            last_device = rustStatus.last_device ?: cachedDevice,
        )
    }

    /** Upload encrypted config to cloud. */
    fun upload(provider: String, masterPassword: String, force: Boolean = false): SyncResponse {
        val params = buildParamsJson(provider, masterPassword, force = force)
        val raw = RustBridge.nativeCloudSyncUpload(params)
        val resp = parseSyncResponse(raw)
        if (resp.ok) {
            writeCache(provider, resp.updated_at, resp.device_name)
        }
        return resp
    }

    /** Download and apply encrypted config from cloud. */
    fun download(
        provider: String,
        masterPassword: String,
        forceDownload: Boolean = false,
    ): SyncResponse {
        val params = buildParamsJson(provider, masterPassword, forceDownload = forceDownload)
        val raw = RustBridge.nativeCloudSyncDownload(params)
        val resp = parseSyncResponse(raw)
        if (resp.ok) {
            writeCache(provider, resp.updated_at, resp.device_name)
        }
        return resp
    }

    /** Disconnect a provider (remove token). */
    fun disconnect(provider: String): Boolean {
        val ok = RustBridge.nativeCloudSyncDisconnect(provider)
        if (ok) clearCache(provider)
        return ok
    }

    // === SECTION cloud_sync_manager_2 END ===

    /** Read cached sync info from SharedPreferences. */
    private fun readCache(provider: String): Pair<String?, String?> {
        val prefs = appContext?.getSharedPreferences(PREFS_NAME, android.content.Context.MODE_PRIVATE)
        if (prefs != null) {
            val synced = prefs.getString("${provider}_last_synced", null)
            val device = prefs.getString("${provider}_last_device", null)
            if (synced != null) return Pair(synced, device)
        }
        return Pair(null, null)
    }

    /** Write sync info to SharedPreferences after successful upload/download. */
    private fun writeCache(provider: String, updatedAt: String?, deviceName: String?) {
        val ctx = appContext ?: return
        val prefs = ctx.getSharedPreferences(PREFS_NAME, android.content.Context.MODE_PRIVATE)
        prefs.edit().apply {
            if (updatedAt != null) putString("${provider}_last_synced", updatedAt)
            if (deviceName != null) putString("${provider}_last_device", deviceName)
        }.apply()
    }

    /** Clear cached sync info for a provider (on disconnect). */
    private fun clearCache(provider: String) {
        val ctx = appContext ?: return
        val prefs = ctx.getSharedPreferences(PREFS_NAME, android.content.Context.MODE_PRIVATE)
        prefs.edit().remove("${provider}_last_synced").remove("${provider}_last_device").apply()
    }

    /** App context for SharedPreferences (set from Application.onCreate). */
    var appContext: android.content.Context? = null

    private fun buildParamsJson(
        provider: String,
        masterPassword: String,
        force: Boolean = false,
        forceDownload: Boolean = false,
    ): String {
        // Use kotlinx.serialization JsonObject to avoid JSON injection from
        // special characters in masterPassword (e.g. ", \, newlines).
        val map = mutableMapOf<String, kotlinx.serialization.json.JsonElement>(
            "provider" to kotlinx.serialization.json.JsonPrimitive(provider),
            "master_password" to kotlinx.serialization.json.JsonPrimitive(masterPassword),
        )
        if (force) map["force"] = kotlinx.serialization.json.JsonPrimitive(true)
        if (forceDownload) map["force_download"] = kotlinx.serialization.json.JsonPrimitive(true)
        return json.encodeToString(JsonObject.serializer(), JsonObject(map))
    }

    private fun parseSyncResponse(raw: String): SyncResponse {
        if (raw.isBlank()) {
            return SyncResponse(ok = false, reason = "error", message = "无响应")
        }
        return runCatching { json.decodeFromString(SyncResponse.serializer(), raw) }
            .onFailure { Log.e(TAG, "parseSyncResponse failed: $it raw=$raw") }
            .getOrElse {
                SyncResponse(ok = false, reason = "parse_error", message = "解析响应失败")
            }
    }
}

// === SECTION cloud_sync_manager_3 END ===
