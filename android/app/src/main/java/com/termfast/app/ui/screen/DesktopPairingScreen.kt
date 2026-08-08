package com.termfast.app.ui.screen

import android.widget.Toast
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Computer
import androidx.compose.material.icons.filled.Link
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.navigation.NavController
import com.termfast.app.data.PairingApi
import com.termfast.app.data.PairingStore
import com.termfast.app.data.RemoteTunnelConfig
import com.termfast.app.data.TunnelState
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.security.SecureRandom

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun DesktopPairingScreen(navController: NavController) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()

    var token by remember { mutableStateOf<String?>(null) }
    var mobilePairings by remember { mutableStateOf<List<PairingApi.DeviceInfo>>(emptyList()) }
    var desktopPairings by remember { mutableStateOf<List<PairingApi.DeviceInfo>>(emptyList()) }
    var selectedDesktops by remember { mutableStateOf<Set<String>>(emptySet()) }
    var loading by remember { mutableStateOf(false) }

    LaunchedEffect(Unit) {
        PairingStore.init(context)
        token = PairingStore.getToken()
        token?.let { tok ->
            scope.launch {
                try {
                    // Load mobile pairings (phone → desktop) to get desktop list
                    val devices = withContext(Dispatchers.IO) {
                        PairingApi.listDevices(tok, PairingApi.getDeviceName())
                    }
                    mobilePairings = devices.filter { it.pairingType == "mobile" }
                    // Load existing desktop pairings
                    desktopPairings = withContext(Dispatchers.IO) {
                        PairingApi.listDevicesByType(tok, "desktop")
                    }
                } catch (_: Exception) {}
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
            // === SECTION 1 END ===
            // Section 2: Desktop list + pair button
            Text("选择两台桌面端建立互配关系", style = MaterialTheme.typography.titleMedium)

            if (mobilePairings.isEmpty()) {
                Text(
                    "还没有已配对的桌面端，请先在设备配对页面扫码配对桌面端",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            } else {
                mobilePairings.forEach { d ->
                    val isSelected = d.pairingId in selectedDesktops
                    Card(
                        onClick = {
                            selectedDesktops = if (isSelected) {
                                selectedDesktops - d.pairingId
                            } else if (selectedDesktops.size < 2) {
                                selectedDesktops + d.pairingId
                            } else {
                                // Replace oldest selection
                                selectedDesktops.drop(1).toSet() + d.pairingId
                            }
                        },
                        modifier = Modifier.fillMaxWidth(),
                        colors = CardDefaults.cardColors(
                            containerColor = if (isSelected) {
                                MaterialTheme.colorScheme.primaryContainer
                            } else {
                                MaterialTheme.colorScheme.surface
                            },
                        ),
                    ) {
                        Row(
                            modifier = Modifier.padding(16.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Checkbox(
                                checked = isSelected,
                                onCheckedChange = null,
                            )
                            Spacer(Modifier.width(8.dp))
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

                Button(
                    onClick = {
                        if (selectedDesktops.size != 2) {
                            Toast.makeText(context, "请选择两台桌面端", Toast.LENGTH_SHORT).show()
                            return@Button
                        }
                        val desktopA = mobilePairings.find { it.pairingId in selectedDesktops }
                        val desktopB = mobilePairings.find { it.pairingId in selectedDesktops && it.pairingId != desktopA?.pairingId }
                        if (desktopA == null || desktopB == null) {
                            Toast.makeText(context, "无法找到选中的桌面端", Toast.LENGTH_SHORT).show()
                            return@Button
                        }
                        scope.launch {
                            performDesktopPairing(
                                context = context,
                                token = token!!,
                                desktopA = desktopA,
                                desktopB = desktopB,
                                onLoading = { loading = it },
                                onSuccess = {
                                    // Refresh desktop pairings list
                                    scope.launch {
                                        try {
                                            desktopPairings = withContext(Dispatchers.IO) {
                                                PairingApi.listDevicesByType(token!!, "desktop")
                                            }
                                        } catch (_: Exception) {}
                                    }
                                    selectedDesktops = emptySet()
                                },
                            )
                        }
                    },
                    enabled = !loading && selectedDesktops.size == 2,
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    if (loading) {
                        CircularProgressIndicator(
                            modifier = Modifier.size(20.dp),
                            strokeWidth = 2.dp,
                            color = MaterialTheme.colorScheme.onPrimary,
                        )
                    } else {
                        Icon(Icons.Filled.Link, contentDescription = null)
                        Spacer(Modifier.width(8.dp))
                        Text("建立互配")
                    }
                }
            }

            HorizontalDivider(modifier = Modifier.padding(vertical = 8.dp))

            // Section 3: Existing desktop pairings + revoke
            Text("已建立的桌面互配", style = MaterialTheme.typography.titleMedium)

            if (desktopPairings.isEmpty()) {
                Text(
                    "暂无桌面互配",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            } else {
                desktopPairings.forEach { d ->
                    Card(
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        Row(
                            modifier = Modifier.padding(16.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Icon(Icons.Filled.Computer, contentDescription = null)
                            Spacer(Modifier.width(12.dp))
                            Column(modifier = Modifier.weight(1f)) {
                                Text(
                                    "${d.desktopName} ↔ ${d.mobileName.ifEmpty { d.deviceId }}",
                                    style = MaterialTheme.typography.bodyMedium,
                                    fontWeight = FontWeight.Medium,
                                )
                                Text(
                                    "状态: ${d.status}",
                                    style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                )
                            }
                            TextButton(
                                onClick = {
                                    scope.launch {
                                        try {
                                            withContext(Dispatchers.IO) {
                                                PairingApi.revoke(token!!, d.pairingId)
                                            }
                                            desktopPairings = desktopPairings.filter { it.pairingId != d.pairingId }
                                            Toast.makeText(context, "已撤销互配", Toast.LENGTH_SHORT).show()
                                        } catch (e: Exception) {
                                            Toast.makeText(context, "撤销失败: ${e.message}", Toast.LENGTH_SHORT).show()
                                        }
                                    }
                                },
                            ) { Text("撤销", color = MaterialTheme.colorScheme.error) }
                        }
                    }
                }
            }
        }
    }
}
// === SECTION 2 END ===

// === SECTION 3: Desktop pairing coordination logic ===

/**
 * Perform desktop-to-desktop pairing coordination.
 *
 * Flow:
 * 1. Call backend initiateDesktopPairing to create pairing record
 * 2. Generate pairing_key (32 random bytes)
 * 3. Call completePairing to get pairing_jwt
 * 4. Send DESKTOP_PAIR frame to Desktop B (server) via its tunnel
 * 5. Send DESKTOP_PAIR frame to Desktop A (client) via its tunnel
 *
 * @param desktopA The client desktop (will connect to B)
 * @param desktopB The server desktop (will accept A's connection)
 */
private suspend fun performDesktopPairing(
    context: android.content.Context,
    token: String,
    desktopA: PairingApi.DeviceInfo,
    desktopB: PairingApi.DeviceInfo,
    onLoading: (Boolean) -> Unit,
    onSuccess: () -> Unit,
) {
    onLoading(true)
    try {
        // 1. Generate pairing key (32 random bytes) — needed before initiate
        //    so the backend can store it for recovery by desktops.
        val pairingKey = ByteArray(32).also { SecureRandom().nextBytes(it) }
        val pairingKeyHex = pairingKey.joinToString("") { "%02x".format(it) }

        // 2. Create pairing via backend (stores pairing_key_hex)
        val result = withContext(Dispatchers.IO) {
            PairingApi.initiateDesktopPairing(
                token = token,
                serverUserId = desktopB.desktopUserId,
                serverDeviceId = desktopB.desktopDeviceId,
                serverName = desktopB.desktopName,
                clientUserId = desktopA.desktopUserId,
                clientDeviceId = desktopA.desktopDeviceId,
                clientName = desktopA.desktopName,
                pairingKeyHex = pairingKeyHex,
            )
        }
        val pairingId = result.getString("pairing_id")

        // 3. Complete pairing to get JWT
        val completeResult = withContext(Dispatchers.IO) {
            PairingApi.completePairing(
                pairingId = pairingId,
                phonePubkey = "",
                deviceId = desktopA.desktopDeviceId,
                mobileName = desktopA.desktopName,
                token = token,
            )
        }
        val pairingJwt = completeResult.optString("pairing_jwt", "")

        // 4. Get tunnel managers for both desktops
        val pairingA = PairingStore.getPairing(desktopA.pairingId)
        val pairingB = PairingStore.getPairing(desktopB.pairingId)

        if (pairingA == null || pairingB == null) {
            Toast.makeText(context, "找不到桌面端配对信息", Toast.LENGTH_SHORT).show()
            return
        }

        // 5. Send DESKTOP_PAIR to Desktop B (server)
        val sentB = sendDesktopPairFrame(pairingB, pairingId, pairingKeyHex, "", desktopA.desktopName, "server")
        // 6. Send DESKTOP_PAIR to Desktop A (client)
        val sentA = sendDesktopPairFrame(pairingA, pairingId, pairingKeyHex, pairingJwt, desktopB.desktopName, "client")

        if (sentA && sentB) {
            Toast.makeText(context, "互配指令已发送", Toast.LENGTH_SHORT).show()
            onSuccess()
        } else {
            Toast.makeText(context, "部分桌面端未在线，互配指令发送失败", Toast.LENGTH_SHORT).show()
        }
    } catch (e: Exception) {
        Toast.makeText(context, "互配失败: ${e.message}", Toast.LENGTH_SHORT).show()
    } finally {
        onLoading(false)
    }
}

/**
 * Send a DESKTOP_PAIR frame to a desktop via its existing tunnel.
 * Ensures the tunnel is connected and protocol is ready before sending.
 * Returns true if the frame was sent successfully.
 */
private suspend fun sendDesktopPairFrame(
    pairing: RemoteTunnelConfig,
    pairingId: String,
    pairingKeyHex: String,
    pairingJwt: String,
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

            // Start tunnel if not connected
            tunnelManager.start()

            // Wait up to 10 seconds for protocol to be ready
            var waited = 0
            while (!tunnelManager.protocolReady.value && waited < 100) {
                Thread.sleep(100)
                waited++
            }

            if (!tunnelManager.protocolReady.value) {
                android.util.Log.w("DesktopPairing", "Tunnel not ready for ${pairing.pairingId}")
                return@withContext false
            }

            // Build DesktopPairMessage JSON payload
            val payload = JSONObject()
                .put("action", "pair")
                .put("pairing_id", pairingId)
                .put("pairing_key_hex", pairingKeyHex)
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
