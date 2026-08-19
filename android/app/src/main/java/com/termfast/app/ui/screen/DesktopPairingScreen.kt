package com.termfast.app.ui.screen

import android.widget.Toast
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Computer
import androidx.compose.material.icons.filled.Link
import androidx.compose.material.icons.filled.QrCodeScanner
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.navigation.NavController
import com.termfast.app.data.PairingApi
import com.termfast.app.data.PairingStore
import com.termfast.app.data.RemoteTunnelConfig
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject

/// Parsed QR code from a desktop in pairing mode.
data class DesktopQrInfo(
    val deviceId: String,
    val deviceName: String,
    val ecdhPublicKey: String,  // base64 X25519 public key
    val userId: Long,
)

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun DesktopPairingScreen(navController: NavController) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()

    var token by remember { mutableStateOf<String?>(null) }
    var desktopPairings by remember { mutableStateOf<List<PairingApi.DeviceInfo>>(emptyList()) }
    var loading by remember { mutableStateOf(false) }
    // Store QR results as JSON strings (rememberSaveable survives navigation)
    var scannedAJson by rememberSaveable { mutableStateOf<String?>(null) }
    var scannedBJson by rememberSaveable { mutableStateOf<String?>(null) }
    var scanTarget by rememberSaveable { mutableStateOf<String?>(null) } // "A" or "B"

    // Parse JSON back to DesktopQrInfo for display
    val scannedA = scannedAJson?.let { parseDesktopQr(it) }
    val scannedB = scannedBJson?.let { parseDesktopQr(it) }

    // Load token and existing desktop pairings
    LaunchedEffect(Unit) {
        PairingStore.init(context)
        token = PairingStore.getToken()
        token?.let { tok ->
            scope.launch {
                try {
                    desktopPairings = withContext(Dispatchers.IO) {
                        PairingApi.listDevicesByType(tok, "desktop")
                    }
                } catch (_: Exception) {}
            }
        }
    }

    // Listen for QR scan results
    val savedStateHandle = navController.currentBackStackEntry?.savedStateHandle
    LaunchedEffect(savedStateHandle) {
        savedStateHandle?.getStateFlow<String?>("qr_result", null)?.collect { content ->
            if (content != null) {
                savedStateHandle.remove<String>("qr_result")
                android.util.Log.d("DesktopPairing", "QR result received, scanTarget=$scanTarget, content=${content.take(80)}")
                if (scanTarget == null) {
                    // No scan in progress — ignore stale result
                    android.util.Log.w("DesktopPairing", "scanTarget is null, ignoring QR result")
                    return@collect
                }
                try {
                    val json = JSONObject(content)
                    val type = json.optString("type", "")
                    android.util.Log.d("DesktopPairing", "QR type=$type")
                    // Accept both "desktop_pair" (dedicated) and "dual" (unified QR)
                    if (type != "desktop_pair" && type != "dual") {
                        Toast.makeText(context, "这不是桌面互联二维码，请让桌面端进入配对模式", Toast.LENGTH_LONG).show()
                        scanTarget = null
                        return@collect
                    }
                    val info = DesktopQrInfo(
                        deviceId = json.getString("device_id"),
                        deviceName = json.optString("device_name", json.optString("desktop_name", json.getString("device_id"))),
                        ecdhPublicKey = json.getString("ecdh_public_key"),
                        userId = json.optLong("user_id", 0),
                    )
                    val infoJson = JSONObject(content).toString()
                    when (scanTarget) {
                        "A" -> scannedAJson = infoJson
                        "B" -> scannedBJson = infoJson
                    }
                    scanTarget = null
                } catch (e: Exception) {
                    Toast.makeText(context, "二维码解析失败: ${e.message}", Toast.LENGTH_SHORT).show()
                }
            }
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("桌面互配") },
                navigationIcon = {
                    IconButton(onClick = { navController.popBackStack() }) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "返回")
                    }
                }
            )
        }
    ) { padding ->
        if (token == null) {
            Box(
                modifier = Modifier.fillMaxSize().padding(padding),
                contentAlignment = Alignment.Center,
            ) {
                Text("请先在设备配对页面登录")
            }
            return@Scaffold
        }

        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .padding(16.dp)
                .verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text("扫码配对桌面互联", style = MaterialTheme.typography.titleMedium)
            Text(
                "让两台桌面端进入配对模式，分别扫码即可建立互联",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )

            // Scan Desktop A
            Card(modifier = Modifier.fillMaxWidth()) {
                Column(modifier = Modifier.padding(16.dp)) {
                    Text("桌面端 A", fontWeight = FontWeight.Medium)
                    Spacer(Modifier.height(8.dp))
                    scannedA?.let { info ->
                        Text("设备: ${info.deviceName}", style = MaterialTheme.typography.bodyMedium)
                        Text("ID: ${info.deviceId}", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        Text("ECDH: ${info.ecdhPublicKey.take(16)}...", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        Spacer(Modifier.height(8.dp))
                        TextButton(onClick = { scannedAJson = null }) { Text("清除") }
                    } ?: run {
                        Button(
                            onClick = {
                                scanTarget = "A"
                                navController.navigate("qr_scanner")
                            },
                            enabled = !loading,
                        ) {
                            Icon(Icons.Filled.QrCodeScanner, contentDescription = null)
                            Spacer(Modifier.width(8.dp))
                            Text("扫码桌面端 A")
                        }
                    }
                }
            }

            // Scan Desktop B
            Card(modifier = Modifier.fillMaxWidth()) {
                Column(modifier = Modifier.padding(16.dp)) {
                    Text("桌面端 B", fontWeight = FontWeight.Medium)
                    Spacer(Modifier.height(8.dp))
                    scannedB?.let { info ->
                        Text("设备: ${info.deviceName}", style = MaterialTheme.typography.bodyMedium)
                        Text("ID: ${info.deviceId}", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        Text("ECDH: ${info.ecdhPublicKey.take(16)}...", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        Spacer(Modifier.height(8.dp))
                        TextButton(onClick = { scannedBJson = null }) { Text("清除") }
                    } ?: run {
                        Button(
                            onClick = {
                                scanTarget = "B"
                                navController.navigate("qr_scanner")
                            },
                            enabled = !loading,
                        ) {
                            Icon(Icons.Filled.QrCodeScanner, contentDescription = null)
                            Spacer(Modifier.width(8.dp))
                            Text("扫码桌面端 B")
                        }
                    }
                }
            }

            // Pair button
            Button(
                onClick = {
                    val a = scannedA
                    val b = scannedB
                    if (a == null || b == null) {
                        Toast.makeText(context, "请先扫码两台桌面端", Toast.LENGTH_SHORT).show()
                        return@Button
                    }
                    if (a.deviceId == b.deviceId) {
                        Toast.makeText(context, "不能配对同一台设备", Toast.LENGTH_SHORT).show()
                        return@Button
                    }
                    scope.launch {
                        performDesktopPairing(
                            context = context,
                            token = token!!,
                            desktopA = a,
                            desktopB = b,
                            onLoading = { loading = it },
                            onSuccess = {
                                scannedAJson = null
                                scannedBJson = null
                                scope.launch {
                                    try {
                                        desktopPairings = withContext(Dispatchers.IO) {
                                            PairingApi.listDevicesByType(token!!, "desktop")
                                        }
                                    } catch (_: Exception) {}
                                }
                            },
                        )
                    }
                },
                enabled = scannedA != null && scannedB != null && !loading,
                modifier = Modifier.fillMaxWidth(),
            ) {
                if (loading) {
                    CircularProgressIndicator(modifier = Modifier.size(20.dp), strokeWidth = 2.dp)
                    Spacer(Modifier.width(8.dp))
                }
                Icon(Icons.Filled.Link, contentDescription = null)
                Spacer(Modifier.width(8.dp))
                Text("建立互联")
            }

            HorizontalDivider(modifier = Modifier.padding(vertical = 8.dp))

            // Existing desktop pairings list
            Text("已建立的互联", style = MaterialTheme.typography.titleMedium)
            if (desktopPairings.isEmpty()) {
                Text(
                    "暂无互联配对",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            } else {
                desktopPairings.forEach { d ->
                    Card(modifier = Modifier.fillMaxWidth()) {
                        Row(
                            modifier = Modifier.padding(16.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Icon(Icons.Filled.Computer, contentDescription = null)
                            Spacer(Modifier.width(12.dp))
                            Column(modifier = Modifier.weight(1f)) {
                                Text(
                                    d.desktopName.ifEmpty { d.desktopDeviceId },
                                    style = MaterialTheme.typography.bodyMedium,
                                    fontWeight = FontWeight.Medium,
                                )
                                Text(
                                    d.desktopDeviceId,
                                    style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                )
                            }
                        }
                    }
                }
            }
        }
    }
}
// === SECTION 1 END ===

// === SECTION 2: Desktop pairing coordination logic ===

/**
 * Parse a saved QR JSON string back into DesktopQrInfo.
 * Used to restore scanned device info after navigation (rememberSaveable).
 */
private fun parseDesktopQr(jsonStr: String): DesktopQrInfo? {
    return try {
        val json = JSONObject(jsonStr)
        DesktopQrInfo(
            deviceId = json.getString("device_id"),
            deviceName = json.optString("device_name", json.optString("desktop_name", json.getString("device_id"))),
            ecdhPublicKey = json.getString("ecdh_public_key"),
            userId = json.optLong("user_id", 0),
        )
    } catch (e: Exception) {
        null
    }
}

/**
 * Perform desktop-to-desktop pairing via ECDH key agreement.
 *
 * Flow:
 * 1. Call backend initiateDesktopPairing with both ECDH public keys (from QR codes)
 * 2. Call completePairing to get pairing_jwt
 * 3. Send DESKTOP_PAIR frame to Desktop B (server) with A's ECDH public key
 * 4. Send DESKTOP_PAIR frame to Desktop A (client) with B's ECDH public key
 * 5. Each desktop computes shared_secret = ECDH(my_private, peer_public) locally
 *
 * @param desktopA The client desktop (scanned QR A)
 * @param desktopB The server desktop (scanned QR B)
 */
private suspend fun performDesktopPairing(
    context: android.content.Context,
    token: String,
    desktopA: DesktopQrInfo,
    desktopB: DesktopQrInfo,
    onLoading: (Boolean) -> Unit,
    onSuccess: () -> Unit,
) {
    onLoading(true)
    var sentA = false
    var sentB = false
    try {
        // 1. Create pairing via backend (with ECDH public keys, no pairing_key_hex needed)
        val result = withContext(Dispatchers.IO) {
            PairingApi.initiateDesktopPairing(
                token = token,
                serverUserId = desktopB.userId,
                serverDeviceId = desktopB.deviceId,
                serverName = desktopB.deviceName,
                clientUserId = desktopA.userId,
                clientDeviceId = desktopA.deviceId,
                clientName = desktopA.deviceName,
                pairingKeyHex = "",  // Empty: using ECDH instead
                serverEcdhKey = desktopB.ecdhPublicKey,
                clientEcdhKey = desktopA.ecdhPublicKey,
            )
        }
        val pairingId = result.getString("pairing_id")

        // 2. Complete pairing to get JWT
        val completeResult = withContext(Dispatchers.IO) {
            PairingApi.completePairing(
                pairingId = pairingId,
                phonePubkey = "",
                deviceId = desktopA.deviceId,
                mobileName = desktopA.deviceName,
                token = token,
            )
        }
        val pairingJwt = completeResult.optString("pairing_jwt", "")

        // 3. Get tunnel managers for both desktops (phone's mobile pairing tunnel)
        val pairingA = PairingStore.getPairingByDeviceId(desktopA.deviceId)
        val pairingB = PairingStore.getPairingByDeviceId(desktopB.deviceId)

        if (pairingA == null || pairingB == null) {
            Toast.makeText(context, "找不到桌面端的 mobile 配对隧道", Toast.LENGTH_SHORT).show()
            return
        }

        // 4. Send DESKTOP_PAIR to both desktops in parallel
        coroutineScope {
            val sentBDeferred = async {
                sendDesktopPairFrame(
                    pairingB, pairingId, "",
                    desktopA.ecdhPublicKey,  // peer_ecdh_public_key: A's key for B
                    "", pairingJwt, desktopA.deviceName, "server",
                )
            }
            val sentADeferred = async {
                sendDesktopPairFrame(
                    pairingA, pairingId, "",
                    desktopB.ecdhPublicKey,  // peer_ecdh_public_key: B's key for A
                    pairingJwt, "", desktopB.deviceName, "client",
                )
            }
            sentB = sentBDeferred.await()
            sentA = sentADeferred.await()
        }

        if (sentA && sentB) {
            Toast.makeText(context, "互联指令已发送", Toast.LENGTH_SHORT).show()
            onSuccess()
        } else {
            Toast.makeText(context, "部分桌面端未在线，指令发送失败。上线后会自动恢复。", Toast.LENGTH_LONG).show()
            onSuccess()  // Still refresh — desktops will recover via ECDH when online
        }
    } catch (e: Exception) {
        Toast.makeText(context, "互联失败: ${e.message}", Toast.LENGTH_SHORT).show()
    } finally {
        onLoading(false)
    }
}
// === SECTION 2 END ===

// === SECTION 3: DESKTOP_PAIR frame sender ===

/**
 * Send a DESKTOP_PAIR frame to a desktop via its existing mobile pairing tunnel.
 * The frame contains the peer's ECDH public key — the desktop computes the shared
 * secret locally using ECDH, no key is transmitted.
 */
private suspend fun sendDesktopPairFrame(
    pairing: RemoteTunnelConfig,
    pairingId: String,
    pairingKeyHex: String,       // legacy, empty for ECDH flow
    peerEcdhPublicKey: String,   // peer's X25519 public key (base64)
    pairingJwt: String,
    peerJwt: String,             // unused, kept for signature compat
    peerName: String,
    role: String,
): Boolean {
    return withContext(Dispatchers.IO) {
        try {
            val tunnelManager = TerminalSessionManager.getOrCreateTunnelManager(
                pairing.pairingId,
                pairing.pairingKey.chunked(2).map { it.toInt(16).toByte() }.toByteArray(),
                pairing.relayUrl,
                pairing.pairingJwt,
            )

            tunnelManager.start()

            var waited = 0
            while (!tunnelManager.protocolReady.value && waited < 100) {
                Thread.sleep(100)
                waited++
            }

            if (!tunnelManager.protocolReady.value) {
                android.util.Log.w("DesktopPairing", "Tunnel not ready for ${pairing.pairingId}")
                return@withContext false
            }

            val payload = JSONObject()
                .put("action", "pair")
                .put("pairing_id", pairingId)
                .put("pairing_key_hex", pairingKeyHex)
                .put("peer_ecdh_public_key", peerEcdhPublicKey)
                .put("pairing_jwt", pairingJwt)
                .put("peer_name", peerName)
                .put("pairing_type", "desktop")
                .put("role", role)
                .put("relay_url", pairing.relayUrl)
                .toString()

            tunnelManager.sendDesktopPair(payload)
        } catch (e: Exception) {
            android.util.Log.e("DesktopPairing", "Failed to send DESKTOP_PAIR to ${pairing.pairingId}", e)
            false
        }
    }
}
// === SECTION 3 END ===
