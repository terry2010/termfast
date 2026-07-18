package com.termfast.app.data

import android.content.Context
import androidx.security.crypto.EncryptedSharedPreferences
import androidx.security.crypto.MasterKey
import com.termfast.app.RustBridge

/**
 * Manages the encrypted credential store lifecycle on Android.
 *
 * - Wraps the native EncryptedFileCredentialStore JNI calls.
 * - Caches the derived key in EncryptedSharedPreferences (backed by
 *   Android Keystore) so the user only enters the master password once
 *   per device (unless they explicitly lock or reset).
 */
object CredentialManager {

    private const val PREFS_NAME = "termfast_cred_key_cache"
    private const val KEY_CACHED = "cached_derived_key"

    private fun prefs(context: Context) =
        EncryptedSharedPreferences.create(
            context,
            PREFS_NAME,
            MasterKey.Builder(context)
                .setKeyScheme(MasterKey.KeyScheme.AES256_GCM)
                .build(),
            EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
            EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM,
        )

    enum class Status {
        PENDING, NEEDS_MIGRATION, LOCKED, UNLOCKED
    }

    fun status(): Status = when (RustBridge.nativeCredentialStatus()) {
        "pending" -> Status.PENDING
        "needs_migration" -> Status.NEEDS_MIGRATION
        "unlocked" -> Status.UNLOCKED
        else -> Status.LOCKED
    }

    fun isUnlocked(): Boolean = RustBridge.nativeCredentialIsUnlocked()

    /** Initialize a new encrypted store with a master password. */
    fun initialize(context: Context, masterPassword: String): Boolean =
        RustBridge.nativeCredentialInitialize(masterPassword).also { ok ->
            if (ok) cacheKey(context)
        }

    /** Unlock with a master password. */
    fun unlock(context: Context, masterPassword: String): Boolean =
        RustBridge.nativeCredentialUnlock(masterPassword).also { ok ->
            if (ok) cacheKey(context)
        }

    /** Try to unlock using the cached derived key (no user prompt). */
    fun tryCachedUnlock(context: Context): Boolean {
        val cached = prefs(context).getString(KEY_CACHED, null) ?: return false
        val ok = RustBridge.nativeCredentialUnlockWithKey(cached)
        if (!ok) {
            // Cached key is stale — delete it so user is prompted next time.
            prefs(context).edit().remove(KEY_CACHED).apply()
        }
        return ok
    }

    /** Lock the store and clear the cached key. */
    fun lock(context: Context) {
        RustBridge.nativeCredentialLock()
        prefs(context).edit().remove(KEY_CACHED).apply()
    }

    /** Migrate a legacy plaintext file to encrypted format. */
    fun migrate(context: Context, masterPassword: String): Boolean =
        RustBridge.nativeCredentialMigrate(masterPassword).also { ok ->
            if (ok) cacheKey(context)
        }

    /** Change the master password. */
    fun changePassword(context: Context, old: String, new: String): Boolean =
        RustBridge.nativeCredentialChangePassword(old, new).also { ok ->
            if (ok) cacheKey(context)
        }

    /** Reset (delete) the encrypted file and clear cache. */
    fun reset(context: Context): Boolean {
        val ok = RustBridge.nativeCredentialReset()
        prefs(context).edit().remove(KEY_CACHED).apply()
        return ok
    }

    fun export(destPath: String): Boolean = RustBridge.nativeCredentialExport(destPath)
    fun import(srcPath: String, masterPassword: String): Boolean = RustBridge.nativeCredentialImport(srcPath, masterPassword)

    /** Cache the current derived key into EncryptedSharedPreferences. */
    fun cacheKey(context: Context) {
        val key = RustBridge.nativeCredentialGetKey() ?: return
        prefs(context).edit().putString(KEY_CACHED, key).apply()
    }
}
