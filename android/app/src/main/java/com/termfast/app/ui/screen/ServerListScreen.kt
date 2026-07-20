package com.termfast.app.ui.screen

import android.app.Activity
import android.net.VpnService
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
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Speed
import androidx.compose.material.icons.filled.Stop
import androidx.compose.material.icons.filled.Terminal
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.*
import androidx.compose.material3.SwipeToDismissBox
import androidx.compose.material3.SwipeToDismissBoxValue
import androidx.compose.material3.rememberSwipeToDismissBoxState
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
import com.termfast.app.data.RustRepository
import com.termfast.app.data.ServerConfig
import com.termfast.app.data.ServerStatus
import com.termfast.app.data.SettingsRepository
import com.termfast.app.service.SshVpnService
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.net.HttpURLConnection
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

    LaunchedEffect(Unit) { refresh() }

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
        },
        floatingActionButton = {
            ExtendedFloatingActionButton(
                onClick = { navController.navigate("server_add") },
                icon = { Icon(Icons.Filled.Add, contentDescription = null) },
                text = { Text("添加服务器") },
                containerColor = MaterialTheme.colorScheme.primaryContainer,
                contentColor = MaterialTheme.colorScheme.onPrimaryContainer,
            )
        }
    ) { padding ->
        if (loading) {
            Box(Modifier.fillMaxSize().padding(padding), contentAlignment = Alignment.Center) {
                CircularProgressIndicator()
            }
        } else if (servers.isEmpty()) {
            EmptyServerState(modifier = Modifier.padding(padding))
        } else {
            Column(modifier = Modifier.fillMaxSize().padding(padding)) {
                LazyColumn(
                modifier = Modifier.fillMaxSize().weight(1f),
                contentPadding = PaddingValues(horizontal = 16.dp, vertical = 12.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp)
            ) {
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
                                        android.util.Log.i("ServerList", "proxy: startProxy returned $startOk for ${server.id} port $socks5Port")
                                        startOk
                                    }
                                    android.util.Log.i("ServerList", "proxy: ok=$ok, clearing starting state for ${server.id}")
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
                        onTerminal = { navController.navigate("terminal/${server.id}") },
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
}

// === SECTION 1 END ===

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
                // VPN toggle button — top right corner
                VpnToggleButton(
                    vpnRunning = vpnRunning,
                    vpnStarting = vpnStarting,
                    onToggle = onVpnToggle,
                )
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
                    icon = if (proxyRunning) Icons.Filled.Stop else Icons.Filled.Cloud,
                    contentDescription = "代理",
                    onClick = onProxyToggle,
                    loading = proxyStarting,
                    tint = if (proxyRunning) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
                )
                // Terminal button
                OutlinedIconButton(
                    icon = Icons.Filled.Terminal,
                    contentDescription = "终端",
                    onClick = onTerminal,
                )
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

@Composable
private fun VpnToggleButton(
    vpnRunning: Boolean,
    vpnStarting: Boolean,
    onToggle: () -> Unit,
) {
    val containerColor = if (vpnRunning || vpnStarting)
        MaterialTheme.colorScheme.errorContainer
    else
        MaterialTheme.colorScheme.primary
    val contentColor = if (vpnRunning || vpnStarting)
        MaterialTheme.colorScheme.onErrorContainer
    else
        MaterialTheme.colorScheme.onPrimary
    Row(
        modifier = Modifier
            .clip(RoundedCornerShape(10.dp))
            .background(containerColor)
            .clickable(enabled = !vpnStarting, onClick = onToggle)
            .padding(horizontal = 12.dp, vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        if (vpnStarting) {
            CircularProgressIndicator(
                modifier = Modifier.size(16.dp),
                strokeWidth = 2.dp,
                color = contentColor,
            )
        } else {
            Icon(
                if (vpnRunning) Icons.Filled.Stop else Icons.Filled.PlayArrow,
                contentDescription = null,
                modifier = Modifier.size(18.dp),
                tint = contentColor,
            )
        }
        Text(
            if (vpnStarting) "连接中" else if (vpnRunning) "停止" else "启动VPN",
            color = contentColor,
            style = MaterialTheme.typography.labelMedium,
            fontWeight = FontWeight.Medium,
        )
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