package com.termfast.app.ui.screen

import android.app.Activity
import android.net.VpnService
import android.widget.Toast
import com.termfast.app.BuildConfig
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Cloud
import androidx.compose.material.icons.filled.Computer
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Devices
import androidx.compose.material.icons.filled.Link
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.QrCodeScanner
import androidx.compose.material.icons.filled.Public
import androidx.compose.material.icons.filled.Shield
import androidx.compose.material.icons.filled.Speed
import androidx.compose.material.icons.filled.Stop
import androidx.compose.material.icons.filled.Terminal
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.*
import androidx.compose.material3.SwipeToDismissBox
import androidx.compose.material3.SwipeToDismissBoxValue
import androidx.compose.material3.rememberSwipeToDismissBoxState
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.navigation.NavController
import com.termfast.app.data.PairingApi
import com.termfast.app.data.PairingStore
import com.termfast.app.data.RustRepository
import com.termfast.app.data.ServerConfig
import com.termfast.app.data.ServerStatus
import com.termfast.app.data.SettingsRepository
import com.termfast.app.service.SshVpnService
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.net.HttpURLConnection
import java.net.URLEncoder
import java.net.URL

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ServerListScreen(navController: NavController) {
    val repo = remember { RustRepository }
    val scope = rememberCoroutineScope()
    val context = LocalContext.current
    val settingsRepo = remember { SettingsRepository(context) }
    var servers by remember { mutableStateOf<List<ServerConfig>>(emptyList()) }
    var statuses by remember { mutableStateOf<Map<String, ServerStatus>>(emptyMap()) }
    var loading by remember { mutableStateOf(true) }
    var vpnRunning by remember { mutableStateOf(SshVpnService.isRunning(context)) }
    var vpnStarting by remember { mutableStateOf(SshVpnService.isStarting(context)) }
    var vpnFailed by remember { mutableStateOf(SshVpnService.isFailed(context)) }
    var vpnError by remember { mutableStateOf(SshVpnService.lastError) }
    var vpnServerId by remember { mutableStateOf(SshVpnService.activeServerId) }
    var pendingVpnServer by remember { mutableStateOf<ServerConfig?>(null) }
    // Per-server proxy running state
    var proxyRunningIds by remember { mutableStateOf<Set<String>>(emptySet()) }
    var proxyStartingIds by remember { mutableStateOf<Set<String>>(emptySet()) }
    var showRemotePicker by remember { mutableStateOf(false) }
    var selectedPairing by remember { mutableStateOf<com.termfast.app.data.RemoteTunnelConfig?>(null) }
    var showLoginPrompt by remember { mutableStateOf(false) }
    var remoteVersion by remember { mutableStateOf(0) }
    // isOnline map: pairingId -> isOnline (from backend /devices)
    var onlineStatus by remember { mutableStateOf<Map<String, Boolean>>(emptyMap()) }
    val isLoggedIn = remember(remoteVersion) { PairingStore.getToken() != null }
    val remotePairings = remember(remoteVersion) {
        if (isLoggedIn) PairingStore.getAllPairings() else emptyList()
    }
    val hasRemoteConfig = remotePairings.isNotEmpty()

    // Desktop interconnection state
    var desktopPairings by remember { mutableStateOf<List<PairingApi.DeviceInfo>>(emptyList()) }
    var showInterconnectIntro by remember { mutableStateOf(false) }
    val showInterconnectButton by remember(remotePairings, desktopPairings) {
        derivedStateOf {
            if (remotePairings.size < 2) return@derivedStateOf false
            val mobilePairedDesktopIds = remotePairings.map { it.desktopDeviceId }.toSet()
            val interconnectedDesktopIds = mutableSetOf<String>()
            desktopPairings.filter { it.status == "completed" }.forEach { dp ->
                interconnectedDesktopIds.add(dp.desktopDeviceId)
                if (dp.pairingType == "desktop") {
                    interconnectedDesktopIds.add(dp.deviceId)
                }
            }
            mobilePairedDesktopIds.any { it !in interconnectedDesktopIds }
        }
    }

    // Login dialog state
    var loginEmail by remember { mutableStateOf("") }
    var loginPassword by remember { mutableStateOf("") }
    var loginLoading by remember { mutableStateOf(false) }

    // Scan button click — navigate to scanner or prompt login
    val onScanClick: () -> Unit = {
        if (isLoggedIn) {
            navController.navigate("qr_scanner")
        } else {
            showLoginPrompt = true
        }
    }

    val vpnLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.StartActivityForResult()
    ) { result ->
        if (result.resultCode == Activity.RESULT_OK) {
            pendingVpnServer?.let { server ->
                val settings = settingsRepo.load()
                val socks5Port = server.proxy?.socks5_port ?: 1080
                SshVpnService.start(context, server.id, settings, socks5Port)
                vpnRunning = true
                vpnServerId = server.id
            }
        }
        pendingVpnServer = null
    }

    fun startVpn(server: ServerConfig) {
        val prepare = VpnService.prepare(context)
        if (prepare != null) {
            pendingVpnServer = server
            vpnLauncher.launch(prepare)
        } else {
            val settings = settingsRepo.load()
            val socks5Port = server.proxy?.socks5_port ?: 1080
            SshVpnService.start(context, server.id, settings, socks5Port)
            vpnRunning = true
            vpnServerId = server.id
        }
    }

    fun refresh() {
        scope.launch {
            withContext(Dispatchers.IO) {
                val list = repo.listServers()
                val st = list.associate { it.id to repo.getServerStatus(it.id) }
                val vpn = SshVpnService.isRunning(context)
                val starting = SshVpnService.isStarting(context)
                val failed = SshVpnService.isFailed(context)
                val err = SshVpnService.lastError
                val sid = SshVpnService.activeServerId
                // Check proxy running state for each server
                val proxyRunning = list.filter { repo.isProxyRunning(it.id) }.map { it.id }.toSet()
                withContext(Dispatchers.Main) {
                    servers = list
                    statuses = st
                    vpnRunning = vpn
                    vpnStarting = starting
                    vpnFailed = failed
                    vpnError = err
                    vpnServerId = sid
                    proxyRunningIds = proxyRunning
                    loading = false
                }
            }
        }
    }

    LaunchedEffect(Unit) {
        PairingStore.init(context)
        // Sync with backend: remove locally-stored pairings that are revoked
        // or no longer exist on the backend. Also load desktop pairings to
        // determine whether to show the "设备互联" button.
        val token = PairingStore.getToken()
        if (token != null) {
            scope.launch {
                try {
                    val localPairings = PairingStore.getAllPairings()
                    if (localPairings.isNotEmpty()) {
                        val backendDevices = withContext(Dispatchers.IO) {
                            PairingApi.listDevices(token)
                        }
                        // Guard: only prune local pairings if backend returned
                        // at least one completed device. If the backend returns
                        // an empty list (e.g. transient network issue, token
                        // about to expire, or DB replication lag), we must NOT
                        // delete all local pairings — that would cause data loss.
                        val completedDevices = backendDevices.filter { it.status == "completed" }
                        if (completedDevices.isNotEmpty()) {
                            val backendPairingIds = completedDevices
                                .map { it.pairingId }
                                .toSet()
                            // Update online status from backend
                            val statusMap = completedDevices
                                .filter { it.pairingType == "mobile" }
                                .associate { it.pairingId to it.isOnline }
                            onlineStatus = statusMap
                            // Remove local pairings that are revoked or missing on backend
                            localPairings.forEach { local ->
                                if (local.pairingId !in backendPairingIds) {
                                    PairingStore.removePairing(local.pairingId)
                                }
                            }
                            // Trigger recomposition of remotePairings
                            remoteVersion++
                        } else {
                            // Backend returned empty list — keep local pairings as-is
                            android.util.Log.w("ServerList", "backend returned 0 completed devices, skipping prune to avoid data loss")
                        }
                    }
                    // Load desktop pairings to check interconnection status
                    desktopPairings = withContext(Dispatchers.IO) {
                        PairingApi.listDevicesByType(token, "desktop")
                    }
                } catch (e: PairingApi.TokenExpiredException) {
                    // Token expired — clear it so the UI shows "not logged in"
                    // and the user is prompted to re-login. Local pairings are
                    // preserved (not deleted) so they reappear after re-login.
                    android.util.Log.w("ServerList", "token expired, clearing local token")
                    PairingStore.clearToken()
                    remoteVersion++
                    Toast.makeText(context, "登录已过期，请重新登录", Toast.LENGTH_LONG).show()
                } catch (e: Exception) {
                    // Backend unreachable — keep local pairings as-is
                    android.util.Log.w("ServerList", "sync pairings failed: ${e.message}")
                }
            }
        }
        refresh()
    }

    // Listen for QR scan result — auto-complete pairing when scanned from ServerList
    val savedStateHandle = navController.currentBackStackEntry?.savedStateHandle
    LaunchedEffect(savedStateHandle) {
        savedStateHandle?.getStateFlow<String?>("qr_result", null)?.collect { content ->
            if (content != null) {
                savedStateHandle.remove<String>("qr_result")
                val token = PairingStore.getToken()
                if (token == null) {
                    Toast.makeText(context, "请先在设备配对页面登录", Toast.LENGTH_SHORT).show()
                    return@collect
                }
                try {
                    val json = org.json.JSONObject(content)
                    val pairingId = json.getString("pairing_id")
                    val pairingKey = json.optString("pairing_key", "")
                    val relayUrl = json.optString("relay_url", "")
                    val desktopName = json.optString("desktop_name", "")
                    scope.launch {
                        try {
                            val result = withContext(Dispatchers.IO) {
                                val deviceName = PairingApi.getDeviceName()
                                PairingApi.completePairing(pairingId, "phone-pubkey", deviceName, deviceName, token)
                            }
                            val status = result.optString("status")
                            if (status == "completed") {
                                val jwt = result.optString("pairing_jwt")
                                val updatedDevices = withContext(Dispatchers.IO) { PairingApi.listDevices(token) }
                                val desktopDeviceId = updatedDevices.find { it.pairingId == pairingId }?.desktopDeviceId ?: ""
                                if (jwt.isNotEmpty() && pairingKey.isNotEmpty() && relayUrl.isNotEmpty()) {
                                    PairingStore.savePairing(
                                        com.termfast.app.data.RemoteTunnelConfig(
                                            pairingId = pairingId,
                                            pairingKey = pairingKey,
                                            relayUrl = relayUrl,
                                            pairingJwt = jwt,
                                            desktopName = desktopName,
                                            desktopDeviceId = desktopDeviceId,
                                        )
                                    )
                                }
                                Toast.makeText(context, "配对成功: $desktopName", Toast.LENGTH_SHORT).show()
                                remoteVersion++
                            } else {
                                Toast.makeText(context, "配对失败: ${result.optString("error", "未知错误")}", Toast.LENGTH_SHORT).show()
                            }
                        } catch (e: Exception) {
                            Toast.makeText(context, "配对失败: ${e.message}", Toast.LENGTH_SHORT).show()
                        }
                    }
                } catch (e: Exception) {
                    Toast.makeText(context, "无效的二维码", Toast.LENGTH_SHORT).show()
                }
            }
        }
    }

    LaunchedEffect(Unit) {
        while (true) {
            kotlinx.coroutines.delay(500)
            val running = SshVpnService.isRunning(context)
            val starting = SshVpnService.isStarting(context)
            val failed = SshVpnService.isFailed(context)
            val err = SshVpnService.lastError
            val sid = SshVpnService.activeServerId
            if (running != vpnRunning || starting != vpnStarting || failed != vpnFailed || err != vpnError || sid != vpnServerId) {
                vpnRunning = running
                vpnStarting = starting
                vpnFailed = failed
                vpnError = err
                vpnServerId = sid
            }
        }
    }

    val lifecycleOwner = LocalLifecycleOwner.current
    DisposableEffect(lifecycleOwner) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_RESUME) {
                vpnRunning = SshVpnService.isRunning(context)
                vpnStarting = SshVpnService.isStarting(context)
                vpnFailed = SshVpnService.isFailed(context)
                vpnError = SshVpnService.lastError
                vpnServerId = SshVpnService.activeServerId
                remoteVersion++
                refresh()
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose { lifecycleOwner.lifecycle.removeObserver(observer) }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("TermFast", fontWeight = FontWeight.Bold) },
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = MaterialTheme.colorScheme.surface,
                ),
            )
        }
    ) { padding ->
        if (loading) {
            Box(Modifier.fillMaxSize().padding(padding), contentAlignment = Alignment.Center) {
                CircularProgressIndicator()
            }
        } else if (servers.isEmpty()) {
            Box(Modifier.fillMaxSize().padding(padding)) {
                EmptyServerState(modifier = Modifier.fillMaxSize())
                // Floating buttons — left: interconnect + scan, right: add server
                Row(
                    modifier = Modifier
                        .align(Alignment.BottomCenter)
                        .fillMaxWidth()
                        .padding(horizontal = 16.dp, vertical = 16.dp),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.Bottom,
                ) {
                    Column(
                        verticalArrangement = Arrangement.spacedBy(12.dp),
                    ) {
                        if (showInterconnectButton) {
                            ExtendedFloatingActionButton(
                                onClick = { showInterconnectIntro = true },
                                icon = { Icon(Icons.Filled.Link, contentDescription = null) },
                                text = { Text("设备互联") },
                                containerColor = MaterialTheme.colorScheme.tertiaryContainer,
                                contentColor = MaterialTheme.colorScheme.onTertiaryContainer,
                            )
                        }
                        ExtendedFloatingActionButton(
                            onClick = onScanClick,
                            icon = { Icon(Icons.Filled.QrCodeScanner, contentDescription = null) },
                            text = { Text("扫码添加设备") },
                            containerColor = MaterialTheme.colorScheme.secondaryContainer,
                            contentColor = MaterialTheme.colorScheme.onSecondaryContainer,
                        )
                    }
                    ExtendedFloatingActionButton(
                        onClick = { navController.navigate("server_add") },
                        icon = { Icon(Icons.Filled.Add, contentDescription = null) },
                        text = { Text("添加服务器") },
                        containerColor = MaterialTheme.colorScheme.primaryContainer,
                        contentColor = MaterialTheme.colorScheme.onPrimaryContainer,
                    )
                }
            }
        } else {
            Box(modifier = Modifier.fillMaxSize().padding(padding)) {
                Column(modifier = Modifier.fillMaxSize()) {
                LazyColumn(
                modifier = Modifier.fillMaxSize(),
                contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 12.dp, bottom = 80.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp)
            ) {
                // Remote paired devices — show each as a card
                if (hasRemoteConfig) {
                    items(remotePairings, key = { "remote_${it.pairingId}" }) { pairing ->
                        RemoteDeviceCard(
                            desktopName = pairing.desktopName.ifEmpty { pairing.pairingId.take(8) },
                            desktopDeviceId = pairing.desktopDeviceId,
                            isOnline = onlineStatus[pairing.pairingId] ?: false,
                            onClick = {
                                selectedPairing = pairing
                                showRemotePicker = true
                            },
                        )
                    }
                }
                items(servers, key = { it.id }) { server ->
                    var testResult by remember { mutableStateOf<String?>(null) }
                    var testing by remember { mutableStateOf(false) }
                    val isThisVpn = vpnServerId == server.id
                    val cardVpnRunning = vpnRunning && isThisVpn
                    val cardVpnStarting = vpnStarting && isThisVpn
                    val cardVpnFailed = vpnFailed && isThisVpn
                    val cardVpnError = if (cardVpnFailed) vpnError else null
                    ServerCard(
                        server = server,
                        status = statuses[server.id],
                        vpnRunning = cardVpnRunning,
                        vpnStarting = cardVpnStarting,
                        vpnFailed = cardVpnFailed,
                        vpnError = cardVpnError,
                        terminalSessionCount = TerminalSessionManager.getSessions(server.id).size,
                        testResult = testResult,
                        testing = testing,
                        onVpnToggle = {
                            if (cardVpnRunning || cardVpnStarting) {
                                // This card's VPN is running — stop it
                                SshVpnService.stop(context)
                                vpnRunning = false
                                vpnStarting = false
                                vpnServerId = ""
                            } else {
                                // Clear previous error and start new connection
                                vpnFailed = false
                                vpnError = null
                                vpnStarting = true
                                vpnServerId = server.id
                                startVpn(server)
                            }
                        },
                        proxyRunning = server.id in proxyRunningIds,
                        proxyStarting = server.id in proxyStartingIds,
                        onProxyToggle = {
                            scope.launch {
                                if (server.id in proxyRunningIds) {
                                    // Stop proxy
                                    withContext(Dispatchers.IO) {
                                        repo.stopProxy(server.id)
                                    }
                                    proxyRunningIds = proxyRunningIds - server.id
                                } else {
                                    // Start proxy — ensure SSH connected first
                                    proxyStartingIds = proxyStartingIds + server.id
                                    val ok = withContext(Dispatchers.IO) {
                                        // Connect SSH if not already connected
                                        val st = repo.getServerStatus(server.id)
                                        if (st.status != "connected") {
                                            val connected = repo.connectServer(server.id)
                                            if (!connected) {
                                                android.util.Log.w("ServerList", "proxy: connectServer failed for ${server.id}")
                                                return@withContext false
                                            }
                                        }
                                        val socks5Port = server.proxy?.socks5_port ?: 1080
                                        val startOk = repo.startProxy(server.id, socks5Port, 0, 0)
                                        if (BuildConfig.DEBUG) android.util.Log.i("ServerList", "proxy: startProxy returned $startOk for ${server.id} port $socks5Port")
                                        startOk
                                    }
                                    if (BuildConfig.DEBUG) android.util.Log.i("ServerList", "proxy: ok=$ok, clearing starting state for ${server.id}")
                                    proxyStartingIds = proxyStartingIds - server.id
                                    if (ok) {
                                        proxyRunningIds = proxyRunningIds + server.id
                                    }
                                }
                            }
                        },
                        onTest = {
                            scope.launch {
                                testing = true
                                testResult = null
                                withContext(Dispatchers.IO) {
                                    try {
                                        var testUrl = server.test_url.ifBlank { "https://google.com" }
                                        // Auto-add https:// if no scheme
                                        if (!testUrl.startsWith("http://") && !testUrl.startsWith("https://")) {
                                            testUrl = "https://$testUrl"
                                        }
                                        val conn = URL(testUrl).openConnection() as HttpURLConnection
                                        conn.connectTimeout = 8000
                                        conn.readTimeout = 8000
                                        conn.instanceFollowRedirects = false
                                        conn.requestMethod = "GET"
                                        val start = System.currentTimeMillis()
                                        val code = conn.responseCode
                                        val latency = System.currentTimeMillis() - start
                                        testResult = if (code in 200..399) {
                                            "✓ $code · ${latency}ms"
                                        } else {
                                            "✗ HTTP $code"
                                        }
                                        conn.disconnect()
                                    } catch (e: Exception) {
                                        testResult = "✗ ${e.message ?: "失败"}"
                                    }
                                }
                                testing = false
                            }
                        },
                        onClick = { navController.navigate("server_detail/${server.id}") },
                        onTerminal = {
                            // Always create a new terminal session
                            navController.navigate("terminal/${server.id}")
                        },
                        onDelete = {
                            scope.launch {
                                withContext(Dispatchers.IO) {
                                    repo.removeServer(server.id)
                                }
                                refresh()
                            }
                        }
                    )
                }
            }
            }
                // Floating buttons — left: interconnect + scan, right: add server
                Row(
                    modifier = Modifier
                        .align(Alignment.BottomCenter)
                        .fillMaxWidth()
                        .padding(horizontal = 16.dp, vertical = 16.dp),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.Bottom,
                ) {
                    Column(
                        verticalArrangement = Arrangement.spacedBy(12.dp),
                    ) {
                        if (showInterconnectButton) {
                            ExtendedFloatingActionButton(
                                onClick = { showInterconnectIntro = true },
                                icon = { Icon(Icons.Filled.Link, contentDescription = null) },
                                text = { Text("设备互联") },
                                containerColor = MaterialTheme.colorScheme.tertiaryContainer,
                                contentColor = MaterialTheme.colorScheme.onTertiaryContainer,
                            )
                        }
                        ExtendedFloatingActionButton(
                            onClick = onScanClick,
                            icon = { Icon(Icons.Filled.QrCodeScanner, contentDescription = null) },
                            text = { Text("扫码添加设备") },
                            containerColor = MaterialTheme.colorScheme.secondaryContainer,
                            contentColor = MaterialTheme.colorScheme.onSecondaryContainer,
                        )
                    }
                    ExtendedFloatingActionButton(
                        onClick = { navController.navigate("server_add") },
                        icon = { Icon(Icons.Filled.Add, contentDescription = null) },
                        text = { Text("添加服务器") },
                        containerColor = MaterialTheme.colorScheme.primaryContainer,
                        contentColor = MaterialTheme.colorScheme.onPrimaryContainer,
                    )
                }
            }
        }

        // Device interconnection intro dialog
        if (showInterconnectIntro) {
            AlertDialog(
                onDismissRequest = { showInterconnectIntro = false },
                title = { Text("设备互联", fontWeight = FontWeight.Bold) },
                text = {
                    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        Text(
                            "设备互联可以让两台已配对的电脑互相访问对方的终端。",
                            style = MaterialTheme.typography.bodyMedium,
                        )
                        Text(
                            "使用步骤：",
                            style = MaterialTheme.typography.bodyMedium,
                            fontWeight = FontWeight.Medium,
                        )
                        Text("1. 选择两台已配对的电脑", style = MaterialTheme.typography.bodySmall)
                        Text("2. 点击「建立互配」", style = MaterialTheme.typography.bodySmall)
                        Text("3. 两台电脑将自动建立加密连接", style = MaterialTheme.typography.bodySmall)
                        Spacer(Modifier.height(4.dp))
                        Text(
                            "建立互联后，你可以在电脑端的左侧列表中看到互联的设备，直接点击即可远程访问对方的终端。",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                },
                confirmButton = {
                    Button(
                        onClick = {
                            showInterconnectIntro = false
                            navController.navigate("desktop_pairing")
                        }
                    ) { Text("知道了") }
                },
                dismissButton = {
                    TextButton(onClick = { showInterconnectIntro = false }) {
                        Text("取消")
                    }
                },
            )
        }

        // Remote terminal picker dialog — when selectedPairing is set,
        // skip desktop list and go directly to terminal list for that pairing
        RemoteTerminalPickerDialog(
            visible = showRemotePicker,
            initialPairing = selectedPairing,
            onTerminalClick = { terminalId, name, pairingId ->
                showRemotePicker = false
                selectedPairing = null
                val encodedName = URLEncoder.encode(name, "UTF-8")
                navController.navigate("remote_terminal/$pairingId/$terminalId/$encodedName")
            },
            onDismiss = {
                showRemotePicker = false
                selectedPairing = null
            },
        )

        // Login dialog — shown when scanning without login
        if (showLoginPrompt) {
            AlertDialog(
                onDismissRequest = { showLoginPrompt = false },
                title = { Text("登录账号", fontWeight = FontWeight.Bold) },
                text = {
                    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        OutlinedTextField(
                            value = loginEmail,
                            onValueChange = { loginEmail = it },
                            label = { Text("邮箱") },
                            modifier = Modifier.fillMaxWidth(),
                            singleLine = true,
                        )
                        OutlinedTextField(
                            value = loginPassword,
                            onValueChange = { loginPassword = it },
                            label = { Text("密码") },
                            modifier = Modifier.fillMaxWidth(),
                            singleLine = true,
                            visualTransformation = androidx.compose.ui.text.input.PasswordVisualTransformation(),
                            keyboardOptions = androidx.compose.foundation.text.KeyboardOptions(
                                keyboardType = androidx.compose.ui.text.input.KeyboardType.Password,
                                imeAction = androidx.compose.ui.text.input.ImeAction.Done,
                            ),
                        )
                        if (loginLoading) {
                            CircularProgressIndicator(
                                modifier = Modifier.align(Alignment.CenterHorizontally),
                            )
                        }
                    }
                },
                confirmButton = {
                    TextButton(
                        onClick = {
                            scope.launch {
                                loginLoading = true
                                try {
                                    val result = withContext(Dispatchers.IO) {
                                        PairingApi.login(loginEmail, loginPassword)
                                    }
                                    val tok = result.getString("access_token")
                                    PairingStore.saveToken(tok)
                                    loginLoading = false
                                    showLoginPrompt = false
                                    remoteVersion++
                                    Toast.makeText(context, "登录成功", Toast.LENGTH_SHORT).show()
                                    navController.navigate("qr_scanner")
                                } catch (e: Exception) {
                                    loginLoading = false
                                    Toast.makeText(context, "登录失败: ${e.message}", Toast.LENGTH_SHORT).show()
                                }
                            }
                        },
                        enabled = !loginLoading && loginEmail.isNotBlank() && loginPassword.length >= 8,
                    ) { Text("登录") }
                },
                dismissButton = {
                    TextButton(onClick = { showLoginPrompt = false }) { Text("取消") }
                },
            )
        }
    }
}

// === SECTION 1 END ===

@Composable
private fun RemoteDeviceCard(
    desktopName: String,
    desktopDeviceId: String,
    isOnline: Boolean,
    onClick: () -> Unit,
) {
    ElevatedCard(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(16.dp),
        colors = CardDefaults.elevatedCardColors(
            containerColor = MaterialTheme.colorScheme.primaryContainer,
            contentColor = MaterialTheme.colorScheme.onPrimaryContainer,
        ),
        elevation = CardDefaults.elevatedCardElevation(defaultElevation = 1.dp),
        onClick = onClick,
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(16.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(
                modifier = Modifier
                    .size(40.dp)
                    .clip(RoundedCornerShape(10.dp))
                    .background(MaterialTheme.colorScheme.primary),
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    Icons.Filled.Computer,
                    contentDescription = null,
                    modifier = Modifier.size(22.dp),
                    tint = MaterialTheme.colorScheme.onPrimary,
                )
            }
            Spacer(Modifier.width(12.dp))
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    desktopName,
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.SemiBold,
                )
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Box(
                        modifier = Modifier
                            .size(6.dp)
                            .background(
                                if (isOnline) MaterialTheme.colorScheme.primary
                                else MaterialTheme.colorScheme.outlineVariant,
                                shape = androidx.compose.foundation.shape.CircleShape
                            )
                    )
                    Spacer(Modifier.width(6.dp))
                    Text(
                        if (isOnline) "在线" else "离线",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onPrimaryContainer.copy(alpha = 0.7f),
                    )
                }
            }
            Icon(
                Icons.Filled.Terminal,
                contentDescription = null,
                modifier = Modifier.size(20.dp),
                tint = MaterialTheme.colorScheme.onPrimaryContainer.copy(alpha = 0.6f),
            )
        }
    }
}

@Composable
private fun EmptyServerState(modifier: Modifier = Modifier) {
    Box(
        modifier = modifier.fillMaxSize(),
        contentAlignment = Alignment.Center
    ) {
        Column(
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(12.dp)
        ) {
            Icon(
                imageVector = Icons.Filled.Computer,
                contentDescription = null,
                modifier = Modifier.size(64.dp),
                tint = MaterialTheme.colorScheme.outline,
            )
            Text(
                "还没有服务器",
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Text(
                "点击右下角按钮添加你的第一台服务器",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.outline,
            )
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class, ExperimentalFoundationApi::class)
@Composable
private fun ServerCard(
    server: ServerConfig,
    status: ServerStatus?,
    vpnRunning: Boolean,
    vpnStarting: Boolean,
    vpnFailed: Boolean = false,
    vpnError: String? = null,
    proxyRunning: Boolean,
    proxyStarting: Boolean = false,
    terminalSessionCount: Int = 0,
    testResult: String?,
    testing: Boolean,
    onVpnToggle: () -> Unit,
    onProxyToggle: () -> Unit,
    onTest: () -> Unit,
    onClick: () -> Unit,
    onTerminal: () -> Unit,
    onDelete: () -> Unit,
) {
    val isConnected = status?.status == "connected"
    var showDeleteDialog by remember { mutableStateOf(false) }
    val dismissState = rememberSwipeToDismissBoxState(
        confirmValueChange = {
            if (it == SwipeToDismissBoxValue.EndToStart) {
                showDeleteDialog = true
            }
            false // Don't actually dismiss, just show dialog
        }
    )

    // Long-press delete confirmation dialog
    if (showDeleteDialog) {
        AlertDialog(
            onDismissRequest = { showDeleteDialog = false },
            title = { Text("删除服务器") },
            text = { Text("确定要删除「${server.name.ifBlank { server.ssh.host }}」吗？") },
            confirmButton = {
                TextButton(
                    onClick = {
                        showDeleteDialog = false
                        onDelete()
                    },
                    colors = ButtonDefaults.textButtonColors(
                        contentColor = MaterialTheme.colorScheme.error,
                    ),
                ) { Text("删除") }
            },
            dismissButton = {
                TextButton(onClick = { showDeleteDialog = false }) { Text("取消") }
            },
        )
    }

    SwipeToDismissBox(
        state = dismissState,
        backgroundContent = {
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .clip(RoundedCornerShape(16.dp))
                    .background(MaterialTheme.colorScheme.errorContainer)
                    .padding(end = 20.dp),
                contentAlignment = Alignment.CenterEnd,
            ) {
                Icon(
                    Icons.Filled.Delete,
                    contentDescription = "删除",
                    tint = MaterialTheme.colorScheme.onErrorContainer,
                    modifier = Modifier.size(28.dp),
                )
            }
        },
        enableDismissFromStartToEnd = false,
    ) {
        val cardColors = CardDefaults.elevatedCardColors(
            containerColor = MaterialTheme.colorScheme.surfaceContainer,
            contentColor = MaterialTheme.colorScheme.onSurface,
        )
        ElevatedCard(
            modifier = Modifier
                .fillMaxWidth()
                .combinedClickable(
                    onClick = onClick,
                    onLongClick = { showDeleteDialog = true },
                ),
            shape = RoundedCornerShape(16.dp),
            colors = cardColors,
            elevation = CardDefaults.elevatedCardElevation(defaultElevation = 1.dp),
        ) {
            Column(modifier = Modifier.padding(16.dp)) {
            // Header row: icon + name + status dot
            Row(
                verticalAlignment = Alignment.CenterVertically,
                modifier = Modifier.fillMaxWidth(),
            ) {
                // Server icon with status-colored background
                Box(
                    modifier = Modifier
                        .size(40.dp)
                        .clip(RoundedCornerShape(10.dp))
                        .background(
                            if (isConnected)
                                MaterialTheme.colorScheme.primaryContainer
                            else
                                MaterialTheme.colorScheme.surfaceVariant
                        ),
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(
                        Icons.Filled.Computer,
                        contentDescription = null,
                        modifier = Modifier.size(22.dp),
                        tint = if (isConnected)
                            MaterialTheme.colorScheme.onPrimaryContainer
                        else
                            MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Spacer(Modifier.width(12.dp))
                Column(modifier = Modifier.weight(1f)) {
                    Text(
                        server.name.ifBlank { server.ssh.host },
                        style = MaterialTheme.typography.titleMedium,
                        fontWeight = FontWeight.SemiBold,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    Text(
                        "${server.ssh.host}:${server.ssh.port}",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                // SSH terminal button — top right corner (icon + text pill)
                Box {
                    Row(
                        modifier = Modifier
                            .clip(RoundedCornerShape(10.dp))
                            .background(MaterialTheme.colorScheme.primary)
                            .clickable(onClick = onTerminal)
                            .padding(horizontal = 12.dp, vertical = 6.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(4.dp),
                    ) {
                        Icon(
                            Icons.Filled.Terminal,
                            contentDescription = null,
                            modifier = Modifier.size(18.dp),
                            tint = MaterialTheme.colorScheme.onPrimary,
                        )
                        Text(
                            "新建SSH终端",
                            color = MaterialTheme.colorScheme.onPrimary,
                            style = MaterialTheme.typography.labelMedium,
                            fontWeight = FontWeight.Medium,
                        )
                    }
                    if (terminalSessionCount > 0) {
                        Badge(
                            modifier = Modifier.align(Alignment.TopEnd),
                            containerColor = MaterialTheme.colorScheme.error,
                            contentColor = MaterialTheme.colorScheme.onError,
                        ) {
                            Text(
                                if (terminalSessionCount > 9) "9+" else terminalSessionCount.toString(),
                                style = MaterialTheme.typography.labelSmall,
                            )
                        }
                    }
                }
            }

            // Error banner — only on the card that failed
            if (vpnFailed && vpnError != null) {
                Spacer(Modifier.height(10.dp))
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(8.dp))
                        .background(MaterialTheme.colorScheme.errorContainer)
                        .padding(horizontal = 12.dp, vertical = 8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    Icon(
                        Icons.Filled.Warning,
                        contentDescription = "错误",
                        tint = MaterialTheme.colorScheme.onErrorContainer,
                        modifier = Modifier.size(18.dp),
                    )
                    Text(
                        vpnError!!,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onErrorContainer,
                    )
                }
            }

            // Exit IP / test result
            if (status?.exit_ip != null || testResult != null) {
                Spacer(Modifier.height(10.dp))
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(8.dp))
                        .background(MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.5f))
                        .padding(horizontal = 10.dp, vertical = 6.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    if (status?.exit_ip != null) {
                        Text(
                            "IP: ${status.exit_ip}",
                            style = MaterialTheme.typography.labelMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    if (status?.exit_ip != null && testResult != null) {
                        Spacer(Modifier.weight(1f))
                    }
                    if (testResult != null) {
                        if (status?.exit_ip != null) Spacer(Modifier.weight(1f))
                        Text(
                            testResult,
                            style = MaterialTheme.typography.labelMedium,
                            color = if (testResult.startsWith("✓"))
                                MaterialTheme.colorScheme.primary
                            else
                                MaterialTheme.colorScheme.error,
                        )
                    }
                }
            }

            Spacer(Modifier.height(12.dp))

            // Action buttons row: proxy+terminal+test on right
            Row(
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Spacer(Modifier.weight(1f))
                // SOCKS5 proxy toggle button
                OutlinedIconButton(
                    icon = if (proxyRunning) Icons.Filled.Stop else Icons.Filled.Public,
                    contentDescription = "代理",
                    onClick = onProxyToggle,
                    loading = proxyStarting,
                    tint = if (proxyRunning) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
                )
                // VPN toggle button with badge
                Box {
                    OutlinedIconButton(
                        icon = if (vpnRunning) Icons.Filled.Stop else Icons.Filled.Shield,
                        contentDescription = "VPN",
                        onClick = onVpnToggle,
                        loading = vpnStarting,
                        tint = if (vpnRunning) MaterialTheme.colorScheme.error
                               else MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                // Test button
                OutlinedIconButton(
                    icon = Icons.Filled.Speed,
                    contentDescription = "测试",
                    onClick = onTest,
                    enabled = !testing,
                    loading = testing,
                )
            }
        }
        }
    }
}

// === SECTION 2 END ===

@Composable
private fun StatusDot(connected: Boolean, running: Boolean) {
    val color = when {
        running -> MaterialTheme.colorScheme.secondary
        connected -> MaterialTheme.colorScheme.primary
        else -> MaterialTheme.colorScheme.outlineVariant
    }
    Box(
        modifier = Modifier
            .size(10.dp)
            .clip(CircleShape)
            .background(color),
    )
}

@Composable
private fun OutlinedIconButton(
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    contentDescription: String,
    onClick: () -> Unit,
    enabled: Boolean = true,
    loading: Boolean = false,
    tint: androidx.compose.ui.graphics.Color = MaterialTheme.colorScheme.onSurfaceVariant,
) {
    OutlinedButton(
        onClick = onClick,
        enabled = enabled,
        shape = RoundedCornerShape(12.dp),
        contentPadding = PaddingValues(0.dp),
        modifier = Modifier.size(44.dp),
    ) {
        if (loading) {
            CircularProgressIndicator(
                modifier = Modifier.size(18.dp),
                strokeWidth = 2.dp,
            )
        } else {
            Icon(
                icon,
                contentDescription = contentDescription,
                modifier = Modifier.size(20.dp),
                tint = if (enabled) tint else MaterialTheme.colorScheme.outlineVariant,
            )
        }
    }
}