package com.termfast.app.ui.screen

import android.widget.Toast
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Computer
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Terminal
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.navigation.NavController
import com.termfast.app.data.PairingStore
import com.termfast.app.data.RemoteTunnelConfig
import com.termfast.app.data.RemoteTunnelManager
import com.termfast.app.data.RustEvent
import com.termfast.app.data.RustRepository
import com.termfast.app.data.TriggerInstance
import com.termfast.app.ui.screen.TerminalSessionManager
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.boolean
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import java.net.URLEncoder

// === SECTION 1 END ===

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun RemoteDetailScreen(navController: NavController, pairingId: String) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var tab by remember { mutableStateOf(0) }

    // Load pairing config
    val pairing = remember(pairingId) {
        PairingStore.getAllPairings().find { it.pairingId == pairingId }
    }

    if (pairing == null) {
        Scaffold(topBar = {
            TopAppBar(
                title = { Text("远程电脑") },
                navigationIcon = {
                    IconButton(onClick = { navController.popBackStack() }) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "返回")
                    }
                },
            )
        }) { padding ->
            Box(
                modifier = Modifier.fillMaxSize().padding(padding),
                contentAlignment = Alignment.Center,
            ) {
                Text("配对信息不存在", color = MaterialTheme.colorScheme.error)
            }
        }
        return
    }

    val desktopName = pairing.desktopName.ifEmpty { pairingId.take(8) }

    // Terminal picker dialog state
    var showTerminalPicker by remember { mutableStateOf(false) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(desktopName, maxLines = 1) },
                navigationIcon = {
                    IconButton(onClick = { navController.popBackStack() }) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "返回")
                    }
                },
                actions = {
                    IconButton(onClick = { showTerminalPicker = true }) {
                        Icon(Icons.Filled.Terminal, contentDescription = "终端")
                    }
                },
            )
        },
    ) { padding ->
        Column(modifier = Modifier.fillMaxSize().padding(padding)) {
            TabRow(
                selectedTabIndex = tab,
                containerColor = MaterialTheme.colorScheme.surface,
            ) {
                Tab(selected = tab == 0, onClick = { tab = 0 }, text = { Text("概览") })
                Tab(selected = tab == 1, onClick = { tab = 1 }, text = { Text("触发器") })
            }
            when (tab) {
                0 -> RemoteOverviewTab(
                    pairing = pairing,
                    onOpenTerminalPicker = { showTerminalPicker = true },
                )
                1 -> RemoteTriggerTab(
                    pairing = pairing,
                )
            }
        }
    }

    // Terminal picker dialog — same dialog used in ServerListScreen
    if (showTerminalPicker) {
        RemoteTerminalPickerDialog(
            visible = true,
            initialPairing = pairing,
            onTerminalClick = { terminalId, name, pid, serverId, serverName ->
                showTerminalPicker = false
                val encodedName = URLEncoder.encode(name, "UTF-8")
                navController.navigate("remote_terminal/$pid/$terminalId/$encodedName/$serverId/$serverName")
            },
            onDismiss = { showTerminalPicker = false },
        )
    }
}

// === SECTION 2 END ===

@Composable
private fun RemoteOverviewTab(
    pairing: RemoteTunnelConfig,
    onOpenTerminalPicker: () -> Unit,
) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        // Desktop info card
        ElevatedCard(
            modifier = Modifier.fillMaxWidth(),
            shape = RoundedCornerShape(16.dp),
        ) {
            Column(modifier = Modifier.padding(20.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Box(
                        modifier = Modifier
                            .size(48.dp)
                            .clip(RoundedCornerShape(12.dp))
                            .background(MaterialTheme.colorScheme.primaryContainer),
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(
                            Icons.Filled.Computer,
                            contentDescription = null,
                            modifier = Modifier.size(28.dp),
                            tint = MaterialTheme.colorScheme.onPrimaryContainer,
                        )
                    }
                    Spacer(Modifier.width(16.dp))
                    Column {
                        Text(
                            pairing.desktopName.ifEmpty { pairing.pairingId.take(8) },
                            style = MaterialTheme.typography.titleLarge,
                            fontWeight = FontWeight.Bold,
                        )
                        Text(
                            "远程桌面",
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
            }
        }

        // Action: open terminal
        ElevatedCard(
            onClick = onOpenTerminalPicker,
            modifier = Modifier.fillMaxWidth(),
            shape = RoundedCornerShape(16.dp),
        ) {
            Row(
                modifier = Modifier.padding(20.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Icon(
                    Icons.Filled.Terminal,
                    contentDescription = null,
                    modifier = Modifier.size(28.dp),
                    tint = MaterialTheme.colorScheme.primary,
                )
                Spacer(Modifier.width(16.dp))
                Column(modifier = Modifier.weight(1f)) {
                    Text(
                        "终端",
                        style = MaterialTheme.typography.titleMedium,
                        fontWeight = FontWeight.SemiBold,
                    )
                    Text(
                        "选择或新建远程终端",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
    }
}

// === SECTION 3 END ===

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun RemoteTriggerTab(
    pairing: RemoteTunnelConfig,
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val pairingId = pairing.pairingId

    val pairingKey = remember(pairing.pairingKey) {
        pairing.pairingKey.chunked(2).map { it.toInt(16).toByte() }.toByteArray()
    }

    // Get or create tunnel manager (shared with terminal picker)
    val tunnelManager = remember(pairingId) {
        TerminalSessionManager.getOrCreateTunnelManager(
            pairingId,
            pairingKey,
            pairing.relayUrl,
            pairing.pairingJwt,
            pairing.pairingRefreshToken,
        )
    }

    var triggers by remember { mutableStateOf<List<TriggerInstance>>(emptyList()) }
    var loading by remember { mutableStateOf(true) }
    var error by remember { mutableStateOf<String?>(null) }
    var protocolReady by remember { mutableStateOf(false) }
    var runningId by remember { mutableStateOf<String?>(null) }
    var showAddDialog by remember { mutableStateOf(false) }
    var editingTrigger by remember { mutableStateOf<TriggerInstance?>(null) }

    // Start tunnel on enter
    LaunchedEffect(tunnelManager) {
        loading = true
        error = null
        tunnelManager.start()
    }

    // Collect protocol state
    LaunchedEffect(tunnelManager) {
        tunnelManager.protocolReady.collect { ready ->
            protocolReady = ready
            if (ready) {
                tunnelManager.sendTriggerListRequest()
            }
        }
    }

    // Listen for Rust FFI events
    LaunchedEffect(tunnelManager) {
        RustRepository.events.collect { event ->
            when (event) {
                is RustEvent.RemoteTunnelReady -> {
                    tunnelManager.onProtocolReady()
                }
                is RustEvent.RemoteTriggerList -> {
                    if (event.pairing_id == pairingId) {
                        triggers = parseTriggerList(event.triggers)
                        loading = false
                        error = null
                    }
                }
                is RustEvent.RemoteTriggerExecResult -> {
                    if (event.pairing_id == pairingId) {
                        runningId = null
                        try {
                            val jsonEl = Json.parseToJsonElement(event.result)
                            val success = jsonEl.jsonObject["success"]?.jsonPrimitive?.boolean ?: false
                            val msg = if (success) "执行成功" else "执行失败"
                            withContext(Dispatchers.Main) {
                                Toast.makeText(context, msg, Toast.LENGTH_SHORT).show()
                            }
                        } catch (_: Exception) {
                            withContext(Dispatchers.Main) {
                                Toast.makeText(context, "执行完成", Toast.LENGTH_SHORT).show()
                            }
                        }
                    }
                }
                is RustEvent.RemoteTerminalError -> {
                    if (event.pairing_id == pairingId) {
                        error = event.error
                        loading = false
                    }
                }
                else -> {}
            }
        }
    }

    Column(modifier = Modifier.fillMaxSize().padding(16.dp)) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("触发器", style = MaterialTheme.typography.titleMedium)
            Button(
                onClick = {
                    editingTrigger = TriggerInstance(id = java.util.UUID.randomUUID().toString())
                    showAddDialog = true
                },
                enabled = protocolReady,
            ) {
                Icon(Icons.Filled.Add, contentDescription = null)
                Text("添加")
            }
        }

        if (!protocolReady) {
            Spacer(Modifier.height(8.dp))
            Text(
                "正在连接远程桌面...",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }

        if (loading && protocolReady) {
            Spacer(Modifier.height(16.dp))
            Box(
                modifier = Modifier.fillMaxWidth(),
                contentAlignment = Alignment.Center,
            ) {
                CircularProgressIndicator()
            }
        }

        error?.let {
            Spacer(Modifier.height(8.dp))
            Text(
                "连接失败: $it",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.error,
            )
        }

        Spacer(Modifier.height(8.dp))
        LazyColumn(
            modifier = Modifier.weight(1f),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            items(triggers, key = { it.id }) { t ->
                RemoteTriggerCard(
                    trigger = t,
                    enabled = protocolReady,
                    running = runningId == t.id,
                    onRun = {
                        if (!protocolReady) return@RemoteTriggerCard
                        runningId = t.id
                        val json = """{"trigger_id":"${t.id}"}"""
                        if (!tunnelManager.sendTriggerExec(json)) {
                            runningId = null
                            Toast.makeText(context, "发送失败", Toast.LENGTH_SHORT).show()
                        }
                    },
                    onEdit = {
                        editingTrigger = t
                        showAddDialog = true
                    },
                    onDelete = {
                        val json = """{"trigger_id":"${t.id}"}"""
                        tunnelManager.sendTriggerRemove(json)
                        triggers = triggers.filter { it.id != t.id }
                    },
                )
            }
        }
    }

    // Add/Edit trigger dialog
    if (showAddDialog && editingTrigger != null) {
        RemoteTriggerEditDialog(
            trigger = editingTrigger!!,
            onDismiss = { showAddDialog = false; editingTrigger = null },
            onSave = { updated ->
                showAddDialog = false
                editingTrigger = null
                val json = Json.encodeToString(TriggerInstance.serializer(), updated)
                if (triggers.any { it.id == updated.id }) {
                    // Update existing
                    tunnelManager.sendTriggerUpdate(json)
                } else {
                    // Add new
                    tunnelManager.sendTriggerAdd(json)
                }
                // Optimistic update — will be corrected by next list response
                triggers = if (triggers.any { it.id == updated.id }) {
                    triggers.map { if (it.id == updated.id) updated else it }
                } else {
                    triggers + updated
                }
            },
        )
    }
}

// === SECTION 4 END ===

@Composable
private fun RemoteTriggerCard(
    trigger: TriggerInstance,
    enabled: Boolean,
    running: Boolean,
    onRun: () -> Unit,
    onEdit: () -> Unit,
    onDelete: () -> Unit,
) {
    ElevatedCard(
        onClick = onEdit,
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(14.dp),
        colors = CardDefaults.elevatedCardColors(
            containerColor = MaterialTheme.colorScheme.surfaceContainer,
        ),
    ) {
        Column(modifier = Modifier.padding(14.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    trigger.name.ifEmpty { "未命名触发器" },
                    style = MaterialTheme.typography.titleSmall,
                    fontWeight = FontWeight.SemiBold,
                    modifier = Modifier.weight(1f),
                )
                if (running) {
                    CircularProgressIndicator(
                        modifier = Modifier.size(20.dp),
                        strokeWidth = 2.dp,
                    )
                } else {
                    IconButton(
                        onClick = onRun,
                        enabled = enabled,
                        modifier = Modifier.size(32.dp),
                    ) {
                        Icon(
                            Icons.Filled.PlayArrow,
                            contentDescription = "执行",
                            tint = if (enabled) MaterialTheme.colorScheme.primary
                            else MaterialTheme.colorScheme.outlineVariant,
                        )
                    }
                }
                IconButton(
                    onClick = onDelete,
                    modifier = Modifier.size(32.dp),
                ) {
                    Icon(
                        Icons.Filled.Delete,
                        contentDescription = "删除",
                        tint = MaterialTheme.colorScheme.error,
                        modifier = Modifier.size(18.dp),
                    )
                }
            }
            Spacer(Modifier.height(4.dp))
            Text(
                trigger.trigger_type,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            if (trigger.commands.isNotEmpty()) {
                Spacer(Modifier.height(4.dp))
                Text(
                    trigger.commands.joinToString("\n"),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 3,
                    overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis,
                )
            }
        }
    }
}

// === SECTION 5 END ===

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun RemoteTriggerEditDialog(
    trigger: TriggerInstance,
    onDismiss: () -> Unit,
    onSave: (TriggerInstance) -> Unit,
) {
    var name by remember { mutableStateOf(trigger.name) }
    var type by remember { mutableStateOf(trigger.trigger_type) }
    var commands by remember { mutableStateOf(trigger.commands.joinToString("\n")) }
    var enabled by remember { mutableStateOf(trigger.enabled) }

    val triggerTypes = listOf(
        "ManualFire", "OnConnect", "OnDisconnect",
        "OnNetworkConnect", "OnNetworkDisconnect", "OnLanIpChange",
        "OnInterval", "OnTerminalOpen", "BeforeTerminalClose",
    )

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(if (trigger.name.isEmpty()) "添加触发器" else "编辑触发器") },
        text = {
            Column(
                modifier = Modifier.fillMaxWidth(),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                OutlinedTextField(
                    value = name,
                    onValueChange = { name = it },
                    label = { Text("名称") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
               ExposedDropdownMenuBox(
                    expanded = false,
                    onExpandedChange = {},
                ) {
                    OutlinedTextField(
                        value = type,
                        onValueChange = { type = it },
                        label = { Text("触发类型") },
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth(),
                    )
                }
                OutlinedTextField(
                    value = commands,
                    onValueChange = { commands = it },
                    label = { Text("命令（每行一条）") },
                    modifier = Modifier.fillMaxWidth().heightIn(min = 80.dp, max = 200.dp),
                )
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Checkbox(checked = enabled, onCheckedChange = { enabled = it })
                    Text("启用")
                }
            }
        },
        confirmButton = {
            TextButton(onClick = {
                val cmdList = commands.split("\n").map { it.trim() }.filter { it.isNotEmpty() }
                onSave(
                    trigger.copy(
                        name = name,
                        trigger_type = type,
                        commands = cmdList,
                        enabled = enabled,
                    )
                )
            }) { Text("保存") }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) { Text("取消") }
        },
    )
}

// === SECTION 6 END ===

private val json = Json { ignoreUnknownKeys = true }

private fun parseTriggerList(jsonStr: String): List<TriggerInstance> {
    return try {
        json.decodeFromString<List<TriggerInstance>>(jsonStr)
    } catch (_: Exception) {
        emptyList()
    }
}

// === SECTION 7 END ===
