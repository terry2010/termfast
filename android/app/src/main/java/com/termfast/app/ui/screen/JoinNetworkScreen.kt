package com.termfast.app.ui.screen

import android.widget.Toast
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Computer
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
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * M2: Join Network screen — mobile user selects two desktop devices to interconnect.
 * The backend creates a JoinBatch and notifies the target desktops via WebSocket.
 * The mobile user can then poll the batch status until it's approved or expired.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun JoinNetworkScreen(navController: NavController, token: String) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()

    var devices by remember { mutableStateOf<List<PairingApi.DeviceInfo>>(emptyList()) }
    var loading by remember { mutableStateOf(false) }
    var selectedA by remember { mutableStateOf<PairingApi.DeviceInfo?>(null) }
    var selectedB by remember { mutableStateOf<PairingApi.DeviceInfo?>(null) }
    var batchId by remember { mutableStateOf<String?>(null) }
    var batchStatus by remember { mutableStateOf<String?>(null) }
    var completing by remember { mutableStateOf(false) }

    // Load devices on mount
    LaunchedEffect(token) {
        loading = true
        try {
            devices = withContext(Dispatchers.IO) {
                PairingApi.listDevicesByType(token, "mobile")
            }
        } catch (_: Exception) {
            // Ignore
        }
        loading = false
    }

    // Poll batch status if we have a batchId.
    // M1: When status becomes "approved", automatically call complete for each pairing.
    LaunchedEffect(batchId) {
        if (batchId == null) return@LaunchedEffect
        val mobileDeviceId = PairingApi.getDeviceName()
        while (true) {
            try {
                val info = withContext(Dispatchers.IO) {
                    PairingApi.getJoinBatchInfo(token, batchId!!, mobileDeviceId)
                }
                val status = info.optString("status")
                batchStatus = "$status (${info.optInt("received_approvals")}/${info.optInt("required_approvals")})"
                if (status == "approved") {
                    // M1: Batch approved by desktop signatures — auto-complete all pairings
                    completing = true
                    val pairingIds = info.optJSONArray("pairing_ids")
                    if (pairingIds != null && pairingIds.length() > 0) {
                        var successCount = 0
                        var failCount = 0
                        for (i in 0 until pairingIds.length()) {
                            val pid = pairingIds.getString(i)
                            try {
                                withContext(Dispatchers.IO) {
                                    PairingApi.completePairing(
                                        pairingId = pid,
                                        phonePubkey = "",
                                        deviceId = mobileDeviceId,
                                        mobileName = PairingApi.getDeviceName(),
                                        token = token,
                                        trustLevel = "full",
                                    )
                                }
                                successCount++
                            } catch (_: Exception) {
                                failCount++
                            }
                        }
                        batchStatus = "completed ($successCount succeeded, $failCount failed)"
                        if (failCount == 0) {
                            Toast.makeText(context, "互联完成，已建立 $successCount 条配对", Toast.LENGTH_SHORT).show()
                        } else {
                            Toast.makeText(context, "部分配对失败：$successCount 成功，$failCount 失败", Toast.LENGTH_LONG).show()
                        }
                    }
                    completing = false
                    break
                }
                if (status == "completed" || status == "expired" || status == "rejected") {
                    break
                }
            } catch (_: Exception) {
                // Ignore polling errors
            }
            kotlinx.coroutines.delay(3000)
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("桌面互联") },
                navigationIcon = {
                    IconButton(onClick = { navController.popBackStack() }) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "返回")
                    }
                }
            )
        }
    ) { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .padding(16.dp)
                .verticalScroll(rememberScrollState()),
        ) {
            Text(
                "选择两台桌面设备进行互联",
                fontSize = 16.sp,
                fontWeight = FontWeight.Medium,
                modifier = Modifier.padding(bottom = 16.dp),
            )

            if (loading) {
                Box(
                    modifier = Modifier.fillMaxWidth().padding(32.dp),
                    contentAlignment = Alignment.Center,
                ) {
                    CircularProgressIndicator()
                }
            } else if (devices.isEmpty()) {
                Text(
                    "暂无已配对的桌面设备",
                    fontSize = 14.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(32.dp),
                )
            } else {
                Text("桌面 A", fontSize = 14.sp, fontWeight = FontWeight.Medium, modifier = Modifier.padding(bottom = 8.dp))
                devices.forEach { device ->
                    DeviceSelectionRow(
                        device = device,
                        selected = selectedA?.pairingId == device.pairingId,
                        onClick = {
                            if (selectedB?.pairingId == device.pairingId) {
                                Toast.makeText(context, "请选择不同的设备", Toast.LENGTH_SHORT).show()
                            } else {
                                selectedA = device
                            }
                        },
                    )
                }

                Spacer(Modifier.height(16.dp))

                Text("桌面 B", fontSize = 14.sp, fontWeight = FontWeight.Medium, modifier = Modifier.padding(bottom = 8.dp))
                devices.forEach { device ->
                    DeviceSelectionRow(
                        device = device,
                        selected = selectedB?.pairingId == device.pairingId,
                        onClick = {
                            if (selectedA?.pairingId == device.pairingId) {
                                Toast.makeText(context, "请选择不同的设备", Toast.LENGTH_SHORT).show()
                            } else {
                                selectedB = device
                            }
                        },
                    )
                }

                Spacer(Modifier.height(24.dp))

                Button(
                    onClick = {
                        if (selectedA == null || selectedB == null) {
                            Toast.makeText(context, "请选择两台设备", Toast.LENGTH_SHORT).show()
                            return@Button
                        }
                        scope.launch {
                            loading = true
                            try {
                                val result = withContext(Dispatchers.IO) {
                                    PairingApi.requestJoinNetwork(
                                        token = token,
                                        desktopAUserId = selectedA!!.desktopUserId,
                                        desktopADeviceId = selectedA!!.desktopDeviceId,
                                        desktopAName = selectedA!!.desktopName,
                                        desktopBUserId = selectedB!!.desktopUserId,
                                        desktopBDeviceId = selectedB!!.desktopDeviceId,
                                        desktopBName = selectedB!!.desktopName,
                                    )
                                }
                                batchId = result.optString("batch_id")
                                batchStatus = "pending_approval (0/${result.optInt("required_approvals")})"
                                Toast.makeText(context, "互联请求已发送，等待桌面批准", Toast.LENGTH_SHORT).show()
                            } catch (e: Exception) {
                                Toast.makeText(context, "请求失败: ${e.message}", Toast.LENGTH_SHORT).show()
                            }
                            loading = false
                        }
                    },
                    enabled = selectedA != null && selectedB != null && batchId == null && !loading,
                    modifier = Modifier.fillMaxWidth(),
                ) { Text("发起互联") }
            }

            // Show batch status
            if (batchId != null) {
                Spacer(Modifier.height(24.dp))
                Card(
                    modifier = Modifier.fillMaxWidth(),
                    colors = CardDefaults.cardColors(
                        containerColor = MaterialTheme.colorScheme.surfaceVariant,
                    ),
                ) {
                    Column(modifier = Modifier.padding(16.dp)) {
                        Text("批次状态", fontSize = 14.sp, fontWeight = FontWeight.Medium)
                        Spacer(Modifier.height(8.dp))
                        Text("批次 ID: $batchId", fontSize = 12.sp)
                        Text("状态: $batchStatus", fontSize = 12.sp)
                        if (completing) {
                            Spacer(Modifier.height(8.dp))
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                CircularProgressIndicator(modifier = Modifier.size(16.dp), strokeWidth = 2.dp)
                                Spacer(Modifier.width(8.dp))
                                Text("正在建立互联配对...", fontSize = 12.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                            }
                        } else {
                            Text("等待桌面设备批准签名...", fontSize = 12.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun DeviceSelectionRow(
    device: PairingApi.DeviceInfo,
    selected: Boolean,
    onClick: () -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        RadioButton(selected = selected, onClick = onClick)
        Column(modifier = Modifier.padding(start = 8.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(Icons.Filled.Computer, contentDescription = null, modifier = Modifier.size(16.dp))
                Spacer(Modifier.width(4.dp))
                Text(device.desktopName, fontSize = 14.sp, fontWeight = FontWeight.Medium)
            }
            Text(
                "设备 ID: ${device.desktopDeviceId}",
                fontSize = 12.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}
