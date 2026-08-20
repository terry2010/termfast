package com.termfast.app.ui.screen

import android.os.Build
import android.widget.Toast
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Computer
import androidx.compose.material.icons.filled.QrCodeScanner
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.navigation.NavController
import com.termfast.app.data.PairingApi
import com.termfast.app.data.PairingStore
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PairingScreen(navController: NavController) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()

    var email by remember { mutableStateOf("") }
    var password by remember { mutableStateOf("") }
    var token by remember { mutableStateOf<String?>(null) }
    var devices by remember { mutableStateOf<List<PairingApi.DeviceInfo>>(emptyList()) }
    var loading by remember { mutableStateOf(false) }

    // M3/D5: Trust level selection — shown before completing pairing
    var showTrustLevelDialog by remember { mutableStateOf(false) }
    var pendingQrData by remember { mutableStateOf<PairingApi.QrData?>(null) }
    var selectedTrustLevel by remember { mutableStateOf("full") } // "local_only" or "full"

    // Init PairingStore and load saved token
    LaunchedEffect(Unit) {
        PairingStore.init(context)
        val saved = PairingStore.getToken()
        if (saved != null) {
            token = saved
            scope.launch {
                try {
                    // Don't filter by mobileDeviceId — the backend already filters
                    // by user ID (from JWT). Filtering by getDeviceName() caused
                    // devices to disappear when getprop returned inconsistent values.
                    devices = withContext(Dispatchers.IO) { PairingApi.listDevices() }
                    android.util.Log.d("PairingScreen", "Loaded ${devices.size} devices")
                    devices.forEach { d ->
                        android.util.Log.d("PairingScreen", "Device: ${d.desktopName}, isOnline=${d.isOnline}, type=${d.pairingType}")
                    }
                } catch (e: PairingApi.TokenExpiredException) {
                    android.util.Log.w("PairingScreen", "token expired, clearing local token")
                    PairingStore.clearToken()
                    token = null
                    Toast.makeText(context, "登录已过期，请重新登录", Toast.LENGTH_LONG).show()
                } catch (e: Exception) {
                    android.util.Log.e("PairingScreen", "Failed to load devices", e)
                    Toast.makeText(context, "加载设备列表失败: ${e.message}", Toast.LENGTH_LONG).show()
                }
            }
        }
    }

    // Listen for QR scan result
    val savedStateHandle = navController.currentBackStackEntry?.savedStateHandle
    LaunchedEffect(savedStateHandle) {
        savedStateHandle?.getStateFlow<String?>("qr_result", null)?.collect { content ->
            if (content != null) {
                savedStateHandle.remove<String>("qr_result")
                if (token == null) {
                    Toast.makeText(context, "请先登录后再扫码配对", Toast.LENGTH_LONG).show()
                    return@collect
                }
                // Parse QR content: {"pairing_id":"xxx","backend_url":"xxx"}
                try {
                    val json = JSONObject(content)
                    val pairingId = json.getString("pairing_id")
                    val pairingKey = json.optString("pairing_key", "")
                    val relayUrl = json.optString("relay_url", "")
                    val desktopName = json.optString("desktop_name", "")
                    val deviceId = json.optString("device_id", "")
                    val ecdhPublicKey = json.optString("ecdh_public_key", "")
                    val userId = json.optLong("user_id", 0)
                    // D5: Show trust level dialog before completing pairing
                    pendingQrData = PairingApi.QrData(pairingId, pairingKey, relayUrl, desktopName, deviceId, ecdhPublicKey, userId)
                    selectedTrustLevel = "full"
                    showTrustLevelDialog = true
                } catch (e: Exception) {
                    Toast.makeText(context, "无效的二维码", Toast.LENGTH_SHORT).show()
                }
            }
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("设备配对") },
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
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            if (token == null) {
                // Login / Register
                Text("登录或注册账号", style = MaterialTheme.typography.titleMedium)
                OutlinedTextField(
                    value = email,
                    onValueChange = { email = it },
                    label = { Text("邮箱") },
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                )
                OutlinedTextField(
                    value = password,
                    onValueChange = { password = it },
                    label = { Text("密码（至少8位）") },
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                    visualTransformation = PasswordVisualTransformation(),
                    keyboardOptions = KeyboardOptions(
                        keyboardType = KeyboardType.Password,
                        imeAction = ImeAction.Done,
                    ),
                )
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    OutlinedButton(
                        onClick = {
                            scope.launch {
                                loading = true
                                try {
                                    withContext(Dispatchers.IO) { PairingApi.register(email, password) }
                                    Toast.makeText(context, "注册成功", Toast.LENGTH_SHORT).show()
                                } catch (e: Exception) {
                                    Toast.makeText(context, "注册失败: ${e.message}", Toast.LENGTH_SHORT).show()
                                }
                                loading = false
                            }
                        },
                        enabled = !loading && email.isNotBlank() && password.length >= 8,
                        modifier = Modifier.weight(1f),
                    ) { Text("注册") }
                    Button(
                        onClick = {
                            scope.launch {
                                loading = true
                                try {
                                    val result = withContext(Dispatchers.IO) { PairingApi.login(email, password) }
                                    val tok = result.getString("access_token")
                                    val refreshToken = result.optString("refresh_token", "")
                                    token = tok
                                    PairingStore.saveToken(tok)
                                    if (refreshToken.isNotEmpty()) {
                                        PairingStore.saveRefreshToken(refreshToken)
                                    }
                                    devices = withContext(Dispatchers.IO) { PairingApi.listDevices() }
                                    Toast.makeText(context, "登录成功", Toast.LENGTH_SHORT).show()
                                } catch (e: Exception) {
                                    Toast.makeText(context, "登录失败: ${e.message}", Toast.LENGTH_SHORT).show()
                                }
                                loading = false
                            }
                        },
                        enabled = !loading && email.isNotBlank() && password.length >= 8,
                        modifier = Modifier.weight(1f),
                    ) { Text("登录") }
                }
                if (loading) {
                    CircularProgressIndicator(modifier = Modifier.align(Alignment.CenterHorizontally))
                }
            } else {
                // Logged in — scan QR to pair
                Text("配对新设备", style = MaterialTheme.typography.titleMedium)
                Text(
                    "在桌面端 TermFast 设置 → 配对 → 配对新设备，生成二维码后用手机扫码完成配对",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )

                Button(
                    onClick = { navController.navigate("qr_scanner") },
                    enabled = !loading,
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Icon(Icons.Filled.QrCodeScanner, contentDescription = null)
                    Spacer(Modifier.width(8.dp))
                    Text("扫码配对")
                }

                // Desktop-to-desktop pairing entry
                OutlinedButton(
                    onClick = { navController.navigate("desktop_pairing") },
                    enabled = !loading,
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Icon(Icons.Filled.Computer, contentDescription = null)
                    Spacer(Modifier.width(8.dp))
                    Text("桌面互配")
                }

                // M2: Join network entry — interconnect two desktops
                OutlinedButton(
                    onClick = { navController.navigate("join_network") },
                    enabled = !loading,
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Icon(Icons.Filled.Computer, contentDescription = null)
                    Spacer(Modifier.width(8.dp))
                    Text("桌面互联")
                }

                if (loading) {
                    CircularProgressIndicator(modifier = Modifier.align(Alignment.CenterHorizontally))
                }

                // Paired devices
                if (devices.isNotEmpty()) {
                    Spacer(Modifier.height(8.dp))
                    Text("已配对设备", style = MaterialTheme.typography.titleMedium)
                    devices.forEach { d ->
                        Card(
                            onClick = {
                                scope.launch {
                                    try {
                                        withContext(Dispatchers.IO) { PairingApi.revoke(token!!, d.pairingId) }
                                        PairingStore.removePairing(d.pairingId)
                                        devices = devices.filter { it.pairingId != d.pairingId }
                                        Toast.makeText(context, "已撤销", Toast.LENGTH_SHORT).show()
                                    } catch (e: Exception) {
                                        Toast.makeText(context, "撤销失败: ${e.message}", Toast.LENGTH_SHORT).show()
                                    }
                                }
                            },
                            modifier = Modifier.fillMaxWidth(),
                        ) {
                            Row(
                                modifier = Modifier.padding(16.dp),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                // Online/offline status indicator
                                Box(
                                    modifier = Modifier
                                        .size(8.dp)
                                        .background(
                                            if (d.isOnline) MaterialTheme.colorScheme.primary
                                            else MaterialTheme.colorScheme.outlineVariant,
                                            shape = androidx.compose.foundation.shape.CircleShape
                                        )
                                )
                                Spacer(Modifier.width(12.dp))
                                Column(modifier = Modifier.weight(1f)) {
                                    Text(d.desktopName.ifEmpty { d.deviceId }, style = MaterialTheme.typography.bodyMedium)
                                    Text(
                                        if (d.isOnline) "在线" else "离线",
                                        style = MaterialTheme.typography.bodySmall,
                                        color = if (d.isOnline) MaterialTheme.colorScheme.primary
                                        else MaterialTheme.colorScheme.onSurfaceVariant,
                                    )
                                }
                                Text("撤销", color = MaterialTheme.colorScheme.error, fontSize = 13.sp)
                            }
                        }
                    }
                }

                Spacer(Modifier.height(16.dp))
                TextButton(
                    onClick = {
                        token = null
                        devices = emptyList()
                        PairingStore.clearToken()
                    },
                    modifier = Modifier.fillMaxWidth(),
                ) { Text("退出登录") }
            }
        }
    }

    // M3/D5: Trust level selection dialog — shown before completing pairing
    if (showTrustLevelDialog && pendingQrData != null) {
        val qr = pendingQrData!!
        AlertDialog(
            onDismissRequest = {
                showTrustLevelDialog = false
                pendingQrData = null
            },
            title = { Text("信任级别") },
            text = {
                Column {
                    Text(
                        "选择此设备的信任级别：",
                        fontSize = 14.sp,
                        modifier = Modifier.padding(bottom = 12.dp),
                    )
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(vertical = 4.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        RadioButton(
                            selected = selectedTrustLevel == "full",
                            onClick = { selectedTrustLevel = "full" },
                        )
                        Column(modifier = Modifier.padding(start = 8.dp)) {
                            Text("全部信任", fontSize = 14.sp, fontWeight = FontWeight.Medium)
                            Text(
                                "允许此设备参与网络互联",
                                fontSize = 12.sp,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(vertical = 4.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        RadioButton(
                            selected = selectedTrustLevel == "local_only",
                            onClick = { selectedTrustLevel = "local_only" },
                        )
                        Column(modifier = Modifier.padding(start = 8.dp)) {
                            Text("仅本机", fontSize = 14.sp, fontWeight = FontWeight.Medium)
                            Text(
                                "仅与本机通信，不参与网络互联",
                                fontSize = 12.sp,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                }
            },
            confirmButton = {
                TextButton(onClick = {
                    // D5: Complete pairing with selected trust_level
                    showTrustLevelDialog = false
                    scope.launch {
                        loading = true
                        try {
                            val result = withContext(Dispatchers.IO) {
                                val deviceName = PairingApi.getDeviceName()
                                PairingApi.completePairing(
                                    qr.pairingId, "phone-pubkey", deviceName, deviceName,
                                    token!!, selectedTrustLevel,
                                )
                            }
                            val status = result.optString("status")
                            if (status == "completed") {
                                val jwt = result.optString("pairing_jwt")
                                val updatedDevices = withContext(Dispatchers.IO) { PairingApi.listDevices() }
                                val desktopDeviceId = updatedDevices.find { it.pairingId == qr.pairingId }?.desktopDeviceId ?: ""
                                if (jwt.isNotEmpty() && qr.pairingKey.isNotEmpty() && qr.relayUrl.isNotEmpty()) {
                                    PairingStore.savePairing(
                                        com.termfast.app.data.RemoteTunnelConfig(
                                            pairingId = qr.pairingId,
                                            pairingKey = qr.pairingKey,
                                            relayUrl = qr.relayUrl,
                                            pairingJwt = jwt,
                                            desktopName = qr.desktopName,
                                            desktopDeviceId = desktopDeviceId,
                                        )
                                    )
                                }
                                Toast.makeText(context, "配对成功", Toast.LENGTH_SHORT).show()
                                devices = updatedDevices
                            } else {
                                Toast.makeText(context, "配对失败: ${result.optString("error", "未知错误")}", Toast.LENGTH_SHORT).show()
                            }
                        } catch (e: Exception) {
                            Toast.makeText(context, "配对失败: ${e.message}", Toast.LENGTH_SHORT).show()
                        }
                        loading = false
                        pendingQrData = null
                    }
                }) { Text("确定") }
            },
            dismissButton = {
                TextButton(onClick = {
                    showTrustLevelDialog = false
                    pendingQrData = null
                }) { Text("取消") }
            },
        )
    }
}
