package com.termfast.app.ui.screen

import android.app.Activity
import android.content.Context
import android.net.VpnService
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Stop
import androidx.compose.material.icons.filled.Terminal
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.navigation.NavController
import com.termfast.app.data.RustRepository
import com.termfast.app.data.ServerConfig
import com.termfast.app.data.SettingsRepository
import com.termfast.app.service.SshVpnService
import com.termfast.app.service.SshVpnTileService
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ServerDetailScreen(navController: NavController, serverId: String) {
    val context = LocalContext.current
    val repo = remember { RustRepository }
    val settingsRepo = remember { SettingsRepository(context) }
    val scope = rememberCoroutineScope()
    var status by remember { mutableStateOf("disconnected") }
    var exitIp by remember { mutableStateOf<String?>(null) }
    var proxyRunning by remember { mutableStateOf(false) }
    var vpnRunning by remember { mutableStateOf(false) }
    var vpnStarting by remember { mutableStateOf(false) }
    var vpnFailed by remember { mutableStateOf(false) }
    var vpnError by remember { mutableStateOf<String?>(null) }
    var tab by remember { mutableStateOf(0) }
    var serverConfig by remember { mutableStateOf<ServerConfig?>(null) }

    fun doStartVpn() {
        val settings = settingsRepo.load()
        val socks5Port = serverConfig?.proxy?.socks5_port ?: 1080
        SshVpnService.start(context, serverId, settings, socks5Port)
        SshVpnTileService.setLastServerId(context, serverId)
        vpnStarting = true
        vpnFailed = false
        vpnError = null
    }

    val vpnLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.StartActivityForResult()
    ) { result ->
        if (result.resultCode == Activity.RESULT_OK) {
            doStartVpn()
        }
    }

    fun toggleVpn() {
        if (vpnRunning || vpnStarting) {
            SshVpnService.stop(context)
            vpnRunning = false
            vpnStarting = false
        } else {
            val prepare = VpnService.prepare(context)
            if (prepare != null) {
                vpnLauncher.launch(prepare)
            } else {
                doStartVpn()
            }
        }
    }

    LaunchedEffect(serverId) {
        withContext(Dispatchers.IO) {
            val s = repo.getServerStatus(serverId)
            val cfg = repo.getConfig()?.servers?.find { it.id == serverId }
            val pr = repo.isProxyRunning(serverId)
            val vpn = SshVpnService.isRunning(context)
            withContext(Dispatchers.Main) {
                status = s.status
                exitIp = s.exit_ip
                serverConfig = cfg
                vpnRunning = vpn
                proxyRunning = pr
            }
        }
    }

    // Poll VPN service state to catch async failures
    LaunchedEffect(Unit) {
        while (true) {
            kotlinx.coroutines.delay(500)
            val running = SshVpnService.isRunning(context)
            val starting = SshVpnService.isStarting(context)
            val failed = SshVpnService.isFailedFor(context, serverId)
            val err = if (failed) SshVpnService.lastError else null
            if (running != vpnRunning || starting != vpnStarting || failed != vpnFailed || err != vpnError) {
                vpnRunning = running
                vpnStarting = starting
                vpnFailed = failed
                vpnError = err
            }
        }
    }

    val lifecycleOwner = LocalLifecycleOwner.current
    DisposableEffect(lifecycleOwner, serverId) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_RESUME) {
                scope.launch {
                    withContext(Dispatchers.IO) {
                        val s = repo.getServerStatus(serverId)
                        val pr = repo.isProxyRunning(serverId)
                        val vpn = SshVpnService.isRunning(context)
                        val starting = SshVpnService.isStarting(context)
                        val failed = SshVpnService.isFailedFor(context, serverId)
                        val err = if (failed) SshVpnService.lastError else null
                        withContext(Dispatchers.Main) {
                            status = s.status
                            exitIp = s.exit_ip
                            vpnRunning = vpn
                            vpnStarting = starting
                            vpnFailed = failed
                            vpnError = err
                            proxyRunning = pr
                        }
                    }
                }
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose { lifecycleOwner.lifecycle.removeObserver(observer) }
    }

    val serverName = serverConfig?.name?.ifBlank { serverConfig?.ssh?.host } ?: "服务器"

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(serverName, fontWeight = FontWeight.SemiBold, maxLines = 1) },
                navigationIcon = {
                    IconButton(onClick = { navController.popBackStack() }) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "返回")
                    }
                },
                actions = {
                    IconButton(onClick = { navController.navigate("terminal/$serverId") }) {
                        Icon(Icons.Filled.Terminal, contentDescription = "终端")
                    }
                    IconButton(onClick = { navController.navigate("server_edit/$serverId") }) {
                        Icon(Icons.Filled.Edit, contentDescription = "编辑")
                    }
                }
            )
        }
    ) { padding ->
        Column(modifier = Modifier.fillMaxSize().padding(padding)) {
            TabRow(
                selectedTabIndex = tab,
                containerColor = MaterialTheme.colorScheme.surface,
            ) {
                Tab(selected = tab == 0, onClick = { tab = 0 }, text = { Text("概览") })
                Tab(selected = tab == 1, onClick = { tab = 1 }, text = { Text("代理") })
                Tab(selected = tab == 2, onClick = { tab = 2 }, text = { Text("触发器") })
            }
            when (tab) {
                0 -> OverviewTab(
                    serverId = serverId,
                    status = status,
                    exitIp = exitIp,
                    proxyRunning = proxyRunning,
                    vpnRunning = vpnRunning,
                    vpnStarting = vpnStarting,
                    vpnFailed = vpnFailed,
                    vpnError = vpnError,
                    onVpnToggle = { toggleVpn() },
                    onTerminal = { navController.navigate("terminal/$serverId") },
                )
                1 -> ProxyTab(
                    serverId = serverId,
                    serverConfig = serverConfig,
                    proxyRunning = proxyRunning,
                    onToggle = { run ->
                        scope.launch {
                            withContext(Dispatchers.IO) {
                                if (run) {
                                    repo.startProxy(serverId, serverConfig?.proxy?.socks5_port ?: 1080, 0, 0)
                                } else {
                                    repo.stopProxy(serverId)
                                }
                            }
                            withContext(Dispatchers.Main) {
                                proxyRunning = run
                            }
                        }
                    },
                    onSaveTestUrl = { url ->
                        scope.launch {
                            withContext(Dispatchers.IO) {
                                serverConfig?.let { cfg ->
                                    repo.saveServer(cfg.copy(test_url = url))
                                }
                            }
                        }
                    }
                )
                2 -> TriggerTab(
                    serverId = serverId,
                    onEdit = { t ->
                        navController.navigate("trigger_edit/${serverId}/${t.id}")
                    }
                )
            }
        }
    }
}

// === SECTION 1 END ===

@Composable
private fun OverviewTab(
    serverId: String,
    status: String,
    exitIp: String?,
    proxyRunning: Boolean,
    vpnRunning: Boolean,
    vpnStarting: Boolean = false,
    vpnFailed: Boolean = false,
    vpnError: String? = null,
    onVpnToggle: () -> Unit,
    onTerminal: () -> Unit,
) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        // Error banner — shown when VPN connection failed
        if (vpnFailed && vpnError != null) {
            Card(
                modifier = Modifier.fillMaxWidth(),
                colors = CardDefaults.cardColors(
                    containerColor = MaterialTheme.colorScheme.errorContainer,
                    contentColor = MaterialTheme.colorScheme.onErrorContainer,
                ),
            ) {
                Row(
                    modifier = Modifier.padding(16.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    Icon(Icons.Filled.Warning, contentDescription = "错误")
                    Text(vpnError!!, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.weight(1f))
                }
            }
        }
        // VPN toggle — large primary button
        Button(
            onClick = onVpnToggle,
            modifier = Modifier.fillMaxWidth().height(52.dp),
            shape = RoundedCornerShape(14.dp),
            enabled = !vpnStarting,
            colors = when {
                vpnRunning -> ButtonDefaults.buttonColors(
                    containerColor = MaterialTheme.colorScheme.errorContainer,
                    contentColor = MaterialTheme.colorScheme.onErrorContainer,
                )
                vpnFailed -> ButtonDefaults.buttonColors(
                    containerColor = MaterialTheme.colorScheme.errorContainer,
                    contentColor = MaterialTheme.colorScheme.onErrorContainer,
                )
                else -> ButtonDefaults.buttonColors(
                    containerColor = MaterialTheme.colorScheme.primary,
                    contentColor = MaterialTheme.colorScheme.onPrimary,
                )
            },
        ) {
            if (vpnStarting) {
                CircularProgressIndicator(
                    modifier = Modifier.size(22.dp),
                    color = MaterialTheme.colorScheme.onPrimary,
                    strokeWidth = 2.dp,
                )
            } else {
                Icon(
                    if (vpnRunning) Icons.Filled.Stop else Icons.Filled.PlayArrow,
                    contentDescription = null,
                    modifier = Modifier.size(22.dp),
                )
            }
            Spacer(Modifier.width(8.dp))
            Text(
                when {
                    vpnStarting -> "连接中..."
                    vpnRunning -> "停止 VPN"
                    vpnFailed -> "重试连接"
                    else -> "启动 VPN"
                },
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.SemiBold,
            )
        }

        // Terminal quick-access button
        OutlinedButton(
            onClick = onTerminal,
            modifier = Modifier.fillMaxWidth().height(48.dp),
            shape = RoundedCornerShape(14.dp),
        ) {
            Icon(Icons.Filled.Terminal, contentDescription = null, modifier = Modifier.size(20.dp))
            Spacer(Modifier.width(8.dp))
            Text("打开 SSH 终端")
        }

        // Status info card
        ElevatedCard(
            modifier = Modifier.fillMaxWidth(),
            shape = RoundedCornerShape(14.dp),
            colors = CardDefaults.elevatedCardColors(
                containerColor = MaterialTheme.colorScheme.surfaceContainer,
            ),
        ) {
            Column(modifier = Modifier.padding(16.dp)) {
                Text("状态信息", style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold)
                Spacer(Modifier.height(12.dp))
                StatusRow(label = "SSH 连接", value = status, positive = status == "connected")
                if (exitIp != null) {
                    HorizontalDivider(modifier = Modifier.padding(vertical = 8.dp), color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.3f))
                    StatusRow(label = "出口 IP", value = exitIp)
                }
                HorizontalDivider(modifier = Modifier.padding(vertical = 8.dp), color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.3f))
                StatusRow(label = "代理", value = if (proxyRunning) "运行中" else "已停止", positive = proxyRunning)
                HorizontalDivider(modifier = Modifier.padding(vertical = 8.dp), color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.3f))
                StatusRow(label = "VPN", value = when {
                    vpnRunning -> "运行中"
                    vpnStarting -> "连接中..."
                    vpnFailed -> "连接失败"
                    else -> "已停止"
                }, positive = vpnRunning)
            }
        }
    }
}

@Composable
private fun StatusRow(label: String, value: String, positive: Boolean? = null) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Text(
            value,
            style = MaterialTheme.typography.bodyMedium,
            fontWeight = FontWeight.Medium,
            color = when (positive) {
                true -> MaterialTheme.colorScheme.primary
                false -> MaterialTheme.colorScheme.onSurfaceVariant
                null -> MaterialTheme.colorScheme.onSurface
            },
        )
    }
}

// === SECTION 2 END ===

@Composable
private fun ProxyTab(
    serverId: String,
    serverConfig: ServerConfig?,
    proxyRunning: Boolean,
    onToggle: (Boolean) -> Unit,
    onSaveTestUrl: (String) -> Unit,
) {
    val scope = rememberCoroutineScope()
    val context = androidx.compose.ui.platform.LocalContext.current
    val repo = remember { com.termfast.app.data.RustRepository }
    var socks5Port by remember { mutableStateOf("1080") }
    var testUrl by remember(serverConfig?.id) {
        mutableStateOf(serverConfig?.test_url ?: "https://google.com")
    }
    var testResult by remember { mutableStateOf<String?>(null) }
    var testing by remember { mutableStateOf(false) }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        // Proxy toggle card
        ElevatedCard(
            modifier = Modifier.fillMaxWidth(),
            shape = RoundedCornerShape(14.dp),
            colors = CardDefaults.elevatedCardColors(
                containerColor = MaterialTheme.colorScheme.surfaceContainer,
            ),
        ) {
            Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Text("SOCKS5 代理", style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold)
                OutlinedTextField(
                    value = socks5Port,
                    onValueChange = { socks5Port = it },
                    label = { Text("SOCKS5 端口") },
                    modifier = Modifier.fillMaxWidth(),
                    shape = RoundedCornerShape(12.dp),
                    singleLine = true,
                )
                Button(
                    onClick = { onToggle(!proxyRunning) },
                    modifier = Modifier.fillMaxWidth().height(48.dp),
                    shape = RoundedCornerShape(12.dp),
                    colors = if (proxyRunning) {
                        ButtonDefaults.buttonColors(
                            containerColor = MaterialTheme.colorScheme.errorContainer,
                            contentColor = MaterialTheme.colorScheme.onErrorContainer,
                        )
                    } else {
                        ButtonDefaults.buttonColors()
                    },
                ) {
                    Text(if (proxyRunning) "停止代理" else "启动代理", fontWeight = FontWeight.Medium)
                }
                if (proxyRunning) {
                    Text(
                        "代理运行中 · 端口 $socks5Port",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.primary,
                    )
                }
                Text(
                    "注意：此功能仅启动本机 SOCKS5 代理端口，不会启动 VPN。如需 VPN 上网，请使用「启动 VPN」按钮。",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }

        // Test URL card
        ElevatedCard(
            modifier = Modifier.fillMaxWidth(),
            shape = RoundedCornerShape(14.dp),
            colors = CardDefaults.elevatedCardColors(
                containerColor = MaterialTheme.colorScheme.surfaceContainer,
            ),
        ) {
            Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Text("代理测试", style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold)
                OutlinedTextField(
                    value = testUrl,
                    onValueChange = { testUrl = it },
                    label = { Text("测试 URL") },
                    modifier = Modifier.fillMaxWidth(),
                    shape = RoundedCornerShape(12.dp),
                    singleLine = true,
                )
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Button(
                        onClick = {
                            scope.launch {
                                testing = true
                                testResult = null
                                val result = kotlinx.coroutines.withTimeoutOrNull(12000L) {
                                    kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) {
                                        try {
                                            val url = testUrl.ifBlank { "https://google.com" }
                                            val conn = java.net.URL(url).openConnection() as java.net.HttpURLConnection
                                            conn.connectTimeout = 8000
                                            conn.readTimeout = 8000
                                            conn.instanceFollowRedirects = false
                                            conn.requestMethod = "GET"
                                            val start = System.currentTimeMillis()
                                            val code = conn.responseCode
                                            val latency = System.currentTimeMillis() - start
                                            conn.disconnect()
                                            if (code in 200..399) {
                                                "✓ $code · ${latency}ms"
                                            } else {
                                                "✗ HTTP $code"
                                            }
                                        } catch (e: Exception) {
                                            "✗ ${e.message ?: "失败"}"
                                        }
                                    }
                                }
                                testResult = result ?: "✗ 超时"
                                testing = false
                            }
                        },
                        enabled = !testing,
                        modifier = Modifier.weight(1f).height(44.dp),
                        shape = RoundedCornerShape(12.dp),
                    ) {
                        if (testing) {
                            CircularProgressIndicator(modifier = Modifier.size(16.dp), strokeWidth = 2.dp)
                        } else {
                            Text("测试")
                        }
                    }
                    OutlinedButton(
                        onClick = { onSaveTestUrl(testUrl.ifBlank { "https://google.com" }) },
                        modifier = Modifier.weight(1f).height(44.dp),
                        shape = RoundedCornerShape(12.dp),
                    ) {
                        Text("保存")
                    }
                }
                if (testResult != null) {
                    Text(
                        testResult!!,
                        style = MaterialTheme.typography.bodySmall,
                        color = if (testResult!!.startsWith("✓"))
                            MaterialTheme.colorScheme.primary
                        else
                            MaterialTheme.colorScheme.error,
                    )
                }
            }
        }
    }
}