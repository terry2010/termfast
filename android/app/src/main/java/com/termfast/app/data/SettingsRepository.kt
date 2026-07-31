package com.termfast.app.data

import android.content.Context
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json

private const val PREFS_NAME = "termfast_settings"
private const val KEY_SETTINGS = "app_settings"

class SettingsRepository(context: Context) {
    private val prefs = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
    private val json = Json { ignoreUnknownKeys = true; encodeDefaults = true }

    fun load(): AppSettings {
        val raw = prefs.getString(KEY_SETTINGS, null) ?: return AppSettings()
        return try {
            json.decodeFromString(raw)
        } catch (e: Exception) {
            AppSettings()
        }
    }

    fun save(settings: AppSettings) {
        // commit() = synchronous write. apply() = async, can lose data on crash.
        prefs.edit().putString(KEY_SETTINGS, json.encodeToString(settings)).commit()
    }
}
