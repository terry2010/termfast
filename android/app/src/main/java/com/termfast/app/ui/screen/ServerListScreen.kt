package com.termfast.app.ui.screen

import android.app.Activity
import android.net.VpnService
import android.widget.Toast
import com.termfast.app.BuildConfig
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.gestures.detectDragGesturesAfterLongPress
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
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
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.zIndex
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
import kotlinx.coroutines.delay
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
    // SSH terminal picker dialog state
    var showSshPicker by remember { mutableStateOf(false) }
    var selectedSshServer by remember { mutableStateOf<Pair<String, String>?>(null) } // (serverId, serverName)
    var showLoginPrompt by remember { mutableStateOf(false) }
    var remoteVersion by remember { mutableStateOf(0) }
    // isOnline map: pairingId -> isOnline (from backend /devices)
    var onlineStatus by remember { mutableStateOf<Map<String, Boolean>>(emptyMap()) }
    val isLoggedIn = remember(remoteVersion) { PairingStore.getToken() != null }
    val remotePairings = remember(remoteVersion) {
        if (isLoggedIn) PairingStore.getAllPairings() else emptyList()
    }
    val hasRemoteConfig = remotePairings.isNotEmpty()

    // --- Drag-to-reorder state ---
    // Unified list: remote pairings first, then SSH servers.
    // Each item has a stable key and a type for callback dispatch.
    data class DragItem(val key: String, val type: String, val id: String)
    val allItems = remember(remotePairings, servers) {
        val remotes = remotePairings.map { DragItem("remote_${it.pairingId}", "remote", it.pairingId) }
        val ssh = servers.map { DragItem(it.id, "ssh", it.id) }
        // Apply saved global order if available
        val globalOrder = RustRepository.getGlobalOrder()
        if (globalOrder.isEmpty()) {
            remotes + ssh
        } else {
            val byKey = (remotes + ssh).associateBy { it.key }
            val ordered = globalOrder.mapNotNull { byKey[it] }
            // Append any items not in saved order (newly added)
            val remaining = (remotes + ssh).filter { it.key !in globalOrder.toSet() }
            ordered + remaining
        }
    }
    // Mutable display list for live reorder during drag
    var displayItems by remember { mutableStateOf(allItems) }
    LaunchedEffect(allItems) { displayItems = allItems }

    var draggedItemKey by remember { mutableStateOf<String?>(null) }
    var dragOffsetY by remember { mutableStateOf(0f) }
    val lazyListState = rememberLazyListState()
    // Show reorder buttons during drag and countdown after drag ends
    var showReorderButtons by remember { mutableStateOf(false) }
    var saveCountdown by remember { mutableStateOf(0) }
    val saveCountdownScope = rememberCoroutineScope()
    var countdownJob by remember { mutableStateOf<kotlinx.coroutines.Job?>(null) }

    fun persistOrder(items: List<DragItem>) {
        // Persist unified global order (handles cross-type reorder)
        val newKeys = items.map { it.key }
        val oldKeys = RustRepository.getGlobalOrder()
        if (newKeys != oldKeys) {
            RustRepository.reorderGlobal(newKeys)
        }
        // Also persist per-type order for backward compat
        val remoteOrder = items.filter { it.type == "remote" }.map { it.id }
        val sshOrder = items.filter { it.type == "ssh" }.map { it.id }
        if (remoteOrder != remotePairings.map { it.pairingId }) {
            PairingStore.reorderPairings(remoteOrder)
            remoteVersion++
        }
        if (sshOrder != servers.map { it.id }) {
            RustRepository.reorderServers(sshOrder)
            servers = repo.listServersOrdered()
        }
    }

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
                val list = repo.listServersOrdered()
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
                            PairingApi.listDevices()
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
                            android.util.Log.i("ServerList", "sync: onlineStatus=$statusMap")
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
                        PairingApi.listDevicesByType("desktop")
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

    // Periodically refresh online status (every 15s) so the UI reflects
    // desktops coming online/offline without requiring a full app restart.
    LaunchedEffect(Unit) {
        while (true) {
            delay(15000)
            val token = PairingStore.getToken() ?: continue
            try {
                val devices = withContext(Dispatchers.IO) { PairingApi.listDevices() }
                val statusMap = devices
                    .filter { it.pairingType == "mobile" && it.status == "completed" }
                    .associate { it.pairingId to it.isOnline }
                onlineStatus = statusMap
            } catch (_: Exception) {
                // Best-effort refresh — ignore errors
            }
        }
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
                                val pairingRefreshToken = result.optString("refresh_token", "")
                                val updatedDevices = withContext(Dispatchers.IO) { PairingApi.listDevices() }
                                val matchedDev = updatedDevices.find { it.pairingId == pairingId }
                                val desktopDeviceId = matchedDev?.desktopDeviceId ?: ""
                                if (jwt.isNotEmpty() && pairingKey.isNotEmpty() && relayUrl.isNotEmpty()) {
                                    PairingStore.savePairing(
                                        com.termfast.app.data.RemoteTunnelConfig(
                                            pairingId = pairingId,
                                            pairingKey = pairingKey,
                                            relayUrl = relayUrl,
                                            pairingJwt = jwt,
                                            desktopName = desktopName,
                                            desktopDeviceId = desktopDeviceId,
                                            pairingRefreshToken = pairingRefreshToken,
                                        )
                                    )
                                }
                                Toast.makeText(context, "配对成功: $desktopName", Toast.LENGTH_SHORT).show()
                                // Update online status from the listDevices result
                                val statusMap = updatedDevices
                                    .filter { it.pairingType == "mobile" }
                                    .associate { it.pairingId to it.isOnline }
                                onlineStatus = statusMap
                                remoteVersion++
                                // The desktop may not have connected to the tunnel yet
                                // at the moment pairing completes. Re-fetch online status
                                // after a short delay to catch the desktop coming online.
                                scope.launch {
                                    delay(3000)
                                    try {
                                        val refreshed = withContext(Dispatchers.IO) { PairingApi.listDevices() }
                                        val refreshedMap = refreshed
                                            .filter { it.pairingType == "mobile" }
                                            .associate { it.pairingId to it.isOnline }
                                        onlineStatus = refreshedMap
                                    } catch (_: Exception) {
                                        // Ignore — best-effort refresh
                                    }
                                }
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
                // Reorder buttons — show during drag and countdown
                if (showReorderButtons) {
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(horizontal = 16.dp, vertical = 8.dp),
                        horizontalArrangement = Arrangement.spacedBy(12.dp),
                    ) {
                        OutlinedButton(
                            onClick = {
                                // Restore default order
                                countdownJob?.cancel()
                                countdownJob = null
                                displayItems = allItems
                                showReorderButtons = false
                                saveCountdown = 0
                                // Clear saved global order
                                RustRepository.reorderGlobal(emptyList())
                            },
                            modifier = Modifier.weight(1f),
                            shape = RoundedCornerShape(12.dp),
                        ) { Text("恢复默认排序") }
                        Button(
                            onClick = {
                                // Save immediately
                                countdownJob?.cancel()
                                countdownJob = null
                                saveCountdown = 0
                                showReorderButtons = false
                                persistOrder(displayItems)
                            },
                            modifier = Modifier.weight(1f),
                            shape = RoundedCornerShape(12.dp),
                        ) {
                            Text(if (saveCountdown > 0) "保存排序($saveCountdown)" else "保存排序")
                        }
                    }
                }
                LazyColumn(
                state = lazyListState,
                modifier = Modifier.fillMaxSize(),
                contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 12.dp, bottom = 80.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp)
            ) {
                items(displayItems, key = { it.key }) { item ->
                    val isDragging = draggedItemKey == item.key
                    // Drag modifier stays on the outer Box — never changes between drag/non-drag,
                    // so pointerInput is not recreated and gesture is not interrupted.
                    val dragModifier = Modifier
                        .fillMaxWidth()
                        .zIndex(if (isDragging) 1f else 0f)
                        .graphicsLayer {
                            if (isDragging) {
                                translationY = dragOffsetY
                                shadowElevation = 8f
                                shape = RoundedCornerShape(16.dp)
                                alpha = 0.9f
                            }
                        }
                        .pointerInput(item.key) {
                            detectDragGesturesAfterLongPress(
                                onDragStart = {
                                    // Cancel any ongoing countdown, keep buttons visible
                                    countdownJob?.cancel()
                                    countdownJob = null
                                    saveCountdown = 0
                                    showReorderButtons = true
                                    draggedItemKey = item.key
                                    dragOffsetY = 0f
                                },
                                onDragEnd = {
                                    // Start 3-second countdown to auto-save
                                    draggedItemKey = null
                                    dragOffsetY = 0f
                                    showReorderButtons = true
                                    saveCountdown = 3
                                    countdownJob = saveCountdownScope.launch {
                                        for (i in 3 downTo 1) {
                                            saveCountdown = i
                                            kotlinx.coroutines.delay(1000)
                                        }
                                        saveCountdown = 0
                                        showReorderButtons = false
                                        persistOrder(displayItems)
                                        countdownJob = null
                                    }
                                },
                                onDragCancel = {
                                    // Revert to original order
                                    countdownJob?.cancel()
                                    countdownJob = null
                                    displayItems = allItems
                                    draggedItemKey = null
                                    dragOffsetY = 0f
                                    showReorderButtons = false
                                    saveCountdown = 0
                                },
                                onDrag = { change, dragAmount ->
                                    change.consume()
                                    dragOffsetY += dragAmount.y
                                    // Live reorder: swap when dragged center crosses target's midpoint
                                    val layoutInfo = lazyListState.layoutInfo
                                    val draggedInfo = layoutInfo.visibleItemsInfo.find { it.key == item.key }
                                    if (draggedInfo != null) {
                                        val draggedCenter = draggedInfo.offset + draggedInfo.size / 2 + dragOffsetY.toInt()
                                        val target = layoutInfo.visibleItemsInfo.firstOrNull { vi ->
                                            if (vi.key == item.key) return@firstOrNull false
                                            val targetMid = vi.offset + vi.size / 2
                                            if (draggedInfo.offset > vi.offset) {
                                                // Dragging up: swap when center crosses target midpoint from below
                                                draggedCenter < targetMid
                                            } else {
                                                // Dragging down: swap when center crosses target midpoint from above
                                                draggedCenter > targetMid
                                            }
                                        }
                                        if (target != null && target.key != item.key) {
                                            // Swap in displayItems
                                            val fromIdx = displayItems.indexOfFirst { it.key == item.key }
                                            val toIdx = displayItems.indexOfFirst { it.key == target.key }
                                            if (fromIdx != -1 && toIdx != -1) {
                                                // Adjust dragOffsetY so the card stays under the finger
                                                val offsetDiff = target.offset - draggedInfo.offset
                                                dragOffsetY -= offsetDiff
                                                displayItems = displayItems.toMutableList().also {
                                                    val moved = it.removeAt(fromIdx)
                                                    it.add(toIdx, moved)
                                                }
                                            }
                                        }
                                    }
                                },
                            )
                        }

                    if (item.type == "remote") {
                        val pairing = remotePairings.find { it.pairingId == item.id } ?: return@items
                        // Wrap in Box with drag modifier — inner card switches SwipeToDismissBox/direct
                        Box(modifier = dragModifier) {
                            RemoteDeviceCard(
                                isDragging = isDragging,
                            desktopName = pairing.desktopName.ifEmpty { pairing.pairingId.take(8) },
                            desktopDeviceId = pairing.desktopDeviceId,
                            isOnline = onlineStatus[pairing.pairingId],
                            terminalSessionCount = TerminalSessionManager.getRemoteSessionCount(pairing.pairingId),
                            onClick = {
                                if (draggedItemKey == null) navController.navigate("remote_detail/${pairing.pairingId}")
                            },
                            onTerminalClick = {
                                if (draggedItemKey == null) {
                                    selectedPairing = pairing
                                    showRemotePicker = true
                                }
                            },
                            onUnpair = {
                                scope.launch {
                                    val token = PairingStore.getToken()
                                    if (token != null) {
                                        withContext(Dispatchers.IO) {
                                            try {
                                                PairingApi.revoke(token, pairing.pairingId)
                                            } catch (e: Exception) {
                                                android.util.Log.w("ServerList", "revoke failed: ${e.message}")
                                            }
                                        }
                                    }
                                    PairingStore.removePairing(pairing.pairingId)
                                    remoteVersion++
                                    Toast.makeText(context, "已解除与「${pairing.desktopName.ifEmpty { pairing.pairingId.take(8) }}」的配对", Toast.LENGTH_SHORT).show()
                                }
                            },
                        )
                        }
                    } else {
                        val server = servers.find { it.id == item.id } ?: return@items
                        var testResult by remember { mutableStateOf<String?>(null) }
                        var testing by remember { mutableStateOf(false) }
                        val isThisVpn = vpnServerId == server.id
                        val cardVpnRunning = vpnRunning && isThisVpn
                        val cardVpnStarting = vpnStarting && isThisVpn
                        val cardVpnFailed = vpnFailed && isThisVpn
                        val cardVpnError = if (cardVpnFailed) vpnError else null
                        Box(modifier = dragModifier) {
                        ServerCard(
                            isDragging = isDragging,
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
                                    SshVpnService.stop(context)
                                    vpnRunning = false
                                    vpnStarting = false
                                    vpnServerId = ""
                                } else {
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
                                        withContext(Dispatchers.IO) {
                                            repo.stopProxy(server.id)
                                        }
                                        proxyRunningIds = proxyRunningIds - server.id
                                    } else {
                                        proxyStartingIds = proxyStartingIds + server.id
                                        val ok = withContext(Dispatchers.IO) {
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
                            onClick = {
                                if (draggedItemKey == null) navController.navigate("server_detail/${server.id}")
                            },
                            onTerminal = {
                                if (draggedItemKey == null) {
                                    selectedSshServer = server.id to server.name.ifBlank { server.ssh.host }
                                    showSshPicker = true
                                }
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
            onTerminalClick = { terminalId, name, pairingId, serverId, serverName ->
                showRemotePicker = false
                selectedPairing = null
                val encodedName = URLEncoder.encode(name, "UTF-8")
                navController.navigate("remote_terminal/$pairingId/$terminalId/$encodedName/$serverId/$serverName")
            },
            onDismiss = {
                showRemotePicker = false
                selectedPairing = null
            },
        )

        // SSH terminal picker dialog — shows existing sessions + new terminal button
        selectedSshServer?.let { (sid, sname) ->
            SshTerminalPickerDialog(
                visible = showSshPicker,
                serverId = sid,
                serverName = sname,
                navController = navController,
                onDismiss = {
                    showSshPicker = false
                    selectedSshServer = null
                },
            )
        }

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
    isDragging: Boolean = false,
    desktopName: String,
    desktopDeviceId: String,
    isOnline: Boolean?,
    terminalSessionCount: Int = 0,
    onClick: () -> Unit,
    onTerminalClick: () -> Unit,
    onUnpair: () -> Unit,
) {
    var showUnpairDialog by remember { mutableStateOf(false) }
    val dismissState = rememberSwipeToDismissBoxState(
        confirmValueChange = {
            if (it == SwipeToDismissBoxValue.EndToStart) {
                showUnpairDialog = true
            }
            false // Don't actually dismiss, just show dialog
        }
    )

    // Swipe-to-unpair confirmation dialog
    if (showUnpairDialog) {
        AlertDialog(
            onDismissRequest = { showUnpairDialog = false },
            title = { Text("解除配对") },
            text = { Text("确定要解除与「$desktopName」的配对吗？解除后将无法远程访问该电脑。") },
            confirmButton = {
                TextButton(
                    onClick = {
                        showUnpairDialog = false
                        onUnpair()
                    },
                    colors = ButtonDefaults.textButtonColors(
                        contentColor = MaterialTheme.colorScheme.error,
                    ),
                ) { Text("解除配对") }
            },
            dismissButton = {
                TextButton(onClick = { showUnpairDialog = false }) { Text("取消") }
            },
        )
    }

    // Card content shared between drag and non-drag modes
    val cardContent: @Composable () -> Unit = {
        ElevatedCard(
            modifier = Modifier
                .fillMaxWidth()
                .clickable(onClick = onClick),
            shape = RoundedCornerShape(16.dp),
            colors = CardDefaults.elevatedCardColors(
                containerColor = MaterialTheme.colorScheme.primaryContainer,
                contentColor = MaterialTheme.colorScheme.onPrimaryContainer,
            ),
            elevation = CardDefaults.elevatedCardElevation(defaultElevation = 1.dp),
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
                    if (isOnline == null) {
                        // Status not yet fetched — show small spinner
                        CircularProgressIndicator(
                            modifier = Modifier.size(12.dp),
                            strokeWidth = 2.dp,
                        )
                    } else {
                        Box(
                            modifier = Modifier
                                .size(6.dp)
                                .background(
                                    if (isOnline) MaterialTheme.colorScheme.primary
                                    else MaterialTheme.colorScheme.outlineVariant,
                                    shape = androidx.compose.foundation.shape.CircleShape
                                )
                        )
                    }
                    Spacer(Modifier.width(6.dp))
                    Text(
                        when (isOnline) {
                            null -> "获取中..."
                            true -> "在线"
                            false -> "离线"
                        },
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onPrimaryContainer.copy(alpha = 0.7f),
                    )
                }
            }
            // Terminal button — opens terminal picker dialog
            Box {
                Row(
                    modifier = Modifier
                        .clip(RoundedCornerShape(10.dp))
                        .background(MaterialTheme.colorScheme.primary)
                        .clickable(onClick = onTerminalClick)
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
                        "打开电脑终端",
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
        }
    }

    // When dragging, render card directly (no SwipeToDismissBox → no red background)
    if (isDragging) {
        cardContent()
    } else {
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
                        contentDescription = "解除配对",
                        tint = MaterialTheme.colorScheme.onErrorContainer,
                        modifier = Modifier.size(28.dp),
                    )
                }
            },
            enableDismissFromStartToEnd = false,
        ) {
            cardContent()
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
    isDragging: Boolean = false,
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

    val cardColors = CardDefaults.elevatedCardColors(
        containerColor = MaterialTheme.colorScheme.surfaceContainer,
        contentColor = MaterialTheme.colorScheme.onSurface,
    )

    // Card content shared between drag and non-drag modes
    val cardContent: @Composable () -> Unit = {
        ElevatedCard(
            modifier = Modifier
                .fillMaxWidth()
                .clickable(onClick = onClick),
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

    // When dragging, render card directly (no SwipeToDismissBox → no red background)
    if (isDragging) {
        cardContent()
    } else {
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
            cardContent()
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