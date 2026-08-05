package com.termfast.app.data

import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import org.json.JSONArray
import org.json.JSONObject
import java.util.concurrent.TimeUnit

object PairingApi {
    private const val BACKEND_URL = "http://sh.zimufan.com:39527"

    private val client = OkHttpClient.Builder()
        .connectTimeout(10, TimeUnit.SECONDS)
        .readTimeout(10, TimeUnit.SECONDS)
        .build()

    private val jsonMedia = "application/json".toMediaType()

    fun register(email: String, password: String): JSONObject {
        val body = JSONObject().put("email", email).put("password", password).toString()
        val resp = client.newCall(Request.Builder()
            .post(body.toRequestBody(jsonMedia))
            .url("$BACKEND_URL/auth/register")
            .build()).execute()
        return JSONObject(resp.body!!.string())
    }

    fun login(email: String, password: String): JSONObject {
        val body = JSONObject().put("email", email).put("password", password).toString()
        val resp = client.newCall(Request.Builder()
            .post(body.toRequestBody(jsonMedia))
            .url("$BACKEND_URL/auth/login")
            .build()).execute()
        val json = JSONObject(resp.body!!.string())
        if (!resp.isSuccessful) {
            val err = json.optString("error", "登录失败 (HTTP ${resp.code})")
            throw Exception(err)
        }
        return json
    }

    fun initiatePairing(token: String, deviceId: String): JSONObject {
        val body = JSONObject().put("desktop_device_id", deviceId).toString()
        val resp = client.newCall(Request.Builder()
            .post(body.toRequestBody(jsonMedia))
            .header("Authorization", "Bearer $token")
            .url("$BACKEND_URL/pair/initiate")
            .build()).execute()
        return JSONObject(resp.body!!.string())
    }

    fun pairStatus(token: String, pairingId: String): JSONObject {
        val resp = client.newCall(Request.Builder()
            .get()
            .header("Authorization", "Bearer $token")
            .url("$BACKEND_URL/pair/status?pairing_id=$pairingId")
            .build()).execute()
        return JSONObject(resp.body!!.string())
    }

    fun listDevices(token: String): List<DeviceInfo> {
        val resp = client.newCall(Request.Builder()
            .get()
            .header("Authorization", "Bearer $token")
            .url("$BACKEND_URL/devices")
            .build()).execute()
        val json = JSONObject(resp.body!!.string())
        val arr = json.optJSONArray("devices") ?: return emptyList()
        val list = mutableListOf<DeviceInfo>()
        for (i in 0 until arr.length()) {
            val d = arr.getJSONObject(i)
            list.add(DeviceInfo(
                pairingId = d.optString("pairing_id"),
                deviceId = d.optString("mobile_device_id", d.optString("pairing_id")),
                status = d.optString("status", "active"),
            ))
        }
        return list
    }

    fun revoke(token: String, pairingId: String) {
        client.newCall(Request.Builder()
            .delete()
            .header("Authorization", "Bearer $token")
            .url("$BACKEND_URL/pair/$pairingId")
            .build()).execute()
    }

    fun completePairing(pairingId: String, phonePubkey: String, deviceId: String): JSONObject {
        val body = JSONObject()
            .put("pairing_id", pairingId)
            .put("phone_pubkey", phonePubkey)
            .put("device_id", deviceId)
            .toString()
        val resp = client.newCall(Request.Builder()
            .post(body.toRequestBody(jsonMedia))
            .url("$BACKEND_URL/pair/complete")
            .build()).execute()
        return JSONObject(resp.body!!.string())
    }

    data class DeviceInfo(
        val pairingId: String,
        val deviceId: String,
        val status: String,
    )
}
