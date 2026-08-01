package com.termfast.app.ui.screen

import android.content.Context
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.navigation.NavController
import com.termfast.app.data.RustRepository
import com.termfast.app.data.TriggerInstance
import com.termfast.app.data.TriggerTemplate
import com.termfast.app.data.TriggerResult
import com.termfast.app.service.NotificationHelper
import com.termfast.app.service.SshVpnService
import java.util.UUID
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TriggerTab(
    serverId: String,
    onEdit: (TriggerInstance) -> Unit,
) {
    val repo = remember { RustRepository }
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var triggers by remember { mutableStateOf<List<TriggerInstance>>(emptyList()) }
    var runningId by remember { mutableStateOf<String?>(null) }
    var result by remember { mutableStateOf<TriggerResult?>(null) }
    var errorMsg by remember { mutableStateOf<String?>(null) }
    var connected by remember { mutableStateOf(false) }
    var showTemplateDialog by remember { mutableStateOf(false) }

    fun refresh() {
        triggers = repo.listTriggers(serverId)
        connected = SshVpnService.isRunning(context) || repo.isProxyRunning(serverId)
    }

    LaunchedEffect(serverId) { refresh() }

    val lifecycleOwner = LocalLifecycleOwner.current
    DisposableEffect(lifecycleOwner) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_RESUME) refresh()
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose { lifecycleOwner.lifecycle.removeObserver(observer) }
    }

    // Poll connection state
    LaunchedEffect(Unit) {
        while (true) {
            kotlinx.coroutines.delay(1000)
            val c = SshVpnService.isRunning(context) || repo.isProxyRunning(serverId)
            if (c != connected) connected = c
        }
    }

    Column(modifier = Modifier.fillMaxSize().padding(16.dp)) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically
        ) {
            Text("触发器", style = MaterialTheme.typography.titleMedium)
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedButton(onClick = { showTemplateDialog = true }) {
                    Text("模板库")
                }
                Button(onClick = { onEdit(TriggerInstance(id = UUID.randomUUID().toString())) }) {
                    Icon(Icons.Filled.Add, contentDescription = null)
                    Text("添加")
                }
            }
        }
        if (!connected) {
            Spacer(Modifier.height(8.dp))
            Text(
                "请先连接 VPN 或代理后再执行触发器",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.error
            )
        }
        Spacer(Modifier.height(8.dp))
        LazyColumn(
            modifier = Modifier.weight(1f),
            verticalArrangement = Arrangement.spacedBy(8.dp)
        ) {
            items(triggers, key = { it.id }) { t ->
                TriggerCard(
                    trigger = t,
                    enabled = connected,
                    onToggle = { enabled ->
                        val updated = triggers.map { if (it.id == t.id) it.copy(enabled = enabled) else it }
                        repo.setServerTriggers(serverId, updated)
                        refresh()
                    },
                    onRun = {
                        if (!connected) {
                            errorMsg = "请先连接 VPN 或代理"
                            return@TriggerCard
                        }
                        errorMsg = null
                        result = null
                        runningId = t.id
                        scope.launch {
                            val r = withContext(Dispatchers.IO) {
                                repo.runTrigger(serverId, t.id)
                            }
                            result = r
                            runningId = null
                            // Send notification based on trigger settings
                            if (r.success && t.notify_on_success) {
                                NotificationHelper.sendTriggerNotification(
                                    context, true, t.name,
                                    "成功执行 ${r.executed_commands}/${r.total_commands} 条命令"
                                )
                            }
                            if (!r.success && t.notify_on_failure) {
                                NotificationHelper.sendTriggerNotification(
                                    context, false, t.name,
                                    r.error ?: "有命令执行失败"
                                )
                            }
                        }
                    },
                    onEdit = { onEdit(t) },
                    onDelete = {
                        val updated = triggers.filter { it.id != t.id }
                        repo.setServerTriggers(serverId, updated)
                        refresh()
                    },
                    running = runningId == t.id
                )
            }
        }
        errorMsg?.let {
            Spacer(Modifier.height(4.dp))
            Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error)
        }
        result?.let { r ->
            Spacer(Modifier.height(8.dp))
            TriggerResultView(r)
        }
    }

    if (showTemplateDialog) {
        TemplateLibraryDialog(
            onDismiss = { showTemplateDialog = false },
        )
    }
}

@Composable
private fun TriggerCard(
    trigger: TriggerInstance,
    enabled: Boolean,
    onToggle: (Boolean) -> Unit,
    onRun: () -> Unit,
    onEdit: () -> Unit,
    onDelete: () -> Unit,
    running: Boolean,
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
                    trigger.name,
                    style = MaterialTheme.typography.titleSmall,
                    fontWeight = FontWeight.SemiBold,
                    modifier = Modifier.weight(1f),
                )
                Switch(checked = trigger.enabled, onCheckedChange = onToggle)
            }
            Text(
                "类型: ${trigger.trigger_type}",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            if (trigger.commands.isNotEmpty()) {
                Text(
                    trigger.commands.first(),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                )
            }
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.End
            ) {
                if (running) {
                    CircularProgressIndicator(modifier = Modifier.size(24.dp), strokeWidth = 2.dp)
                    Spacer(Modifier.width(8.dp))
                }
                IconButton(onClick = onRun, enabled = enabled && !running) {
                    Icon(Icons.Filled.PlayArrow, contentDescription = "执行")
                }
                IconButton(onClick = onDelete) {
                    Icon(Icons.Filled.Delete, contentDescription = "删除", tint = MaterialTheme.colorScheme.error)
                }
            }
        }
    }
}

@Composable
private fun TriggerResultView(r: TriggerResult) {
    val header = if (r.success) {
        "✓ 成功: ${r.executed_commands}/${r.total_commands} 命令"
    } else {
        "✗ 失败: ${r.error ?: "有命令执行失败"}"
    }
    ElevatedCard(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(14.dp),
        colors = CardDefaults.elevatedCardColors(
            containerColor = MaterialTheme.colorScheme.surfaceContainer,
        ),
    ) {
        Column(modifier = Modifier.padding(14.dp)) {
            Text(
                header,
                style = MaterialTheme.typography.titleSmall,
                fontWeight = FontWeight.SemiBold,
                color = if (r.success) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.error,
            )
            Spacer(Modifier.height(6.dp))
            r.results.forEach { cmd ->
                Text(
                    "$ ${cmd.command}",
                    style = MaterialTheme.typography.bodySmall,
                    fontFamily = FontFamily.Monospace,
                    color = MaterialTheme.colorScheme.onSurface,
                )
                Text(
                    "退出码: ${cmd.exit_code}",
                    style = MaterialTheme.typography.bodySmall,
                    color = if (cmd.success) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.error
                )
                if (cmd.stdout.isNotBlank()) {
                    Text(
                        "stdout:\n${cmd.stdout.trim()}",
                        style = MaterialTheme.typography.bodySmall,
                        fontFamily = FontFamily.Monospace,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                if (cmd.stderr.isNotBlank()) {
                    Text(
                        "stderr:\n${cmd.stderr.trim()}",
                        style = MaterialTheme.typography.bodySmall,
                        fontFamily = FontFamily.Monospace,
                        color = MaterialTheme.colorScheme.error,
                    )
                }
                Spacer(Modifier.height(8.dp))
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TriggerEditScreen(navController: NavController, serverId: String, triggerId: String?) {
    val repo = remember { RustRepository }
    var name by remember { mutableStateOf("") }
    var type by remember { mutableStateOf("ManualFire") }
    var commands by remember { mutableStateOf("") }
    var enabled by remember { mutableStateOf(true) }
    var timeout by remember { mutableStateOf("30") }
    var notifyOnSuccess by remember { mutableStateOf(false) }
    var notifyOnFailure by remember { mutableStateOf(true) }

    LaunchedEffect(serverId, triggerId) {
        if (triggerId != null) {
            val list = repo.listTriggers(serverId)
            list.find { it.id == triggerId }?.let { t ->
                name = t.name
                type = t.trigger_type
                commands = t.commands.joinToString("\n")
                enabled = t.enabled
                timeout = t.timeout_secs.toString()
                notifyOnSuccess = t.notify_on_success
                notifyOnFailure = t.notify_on_failure
            }
        }
    }

    val templates = remember { repo.listTriggerTemplates() }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(if (triggerId == null) "添加触发器" else "编辑触发器", fontWeight = FontWeight.SemiBold) },
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
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 16.dp, vertical = 12.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp)
        ) {
            ElevatedCard(
                modifier = Modifier.fillMaxWidth(),
                shape = RoundedCornerShape(16.dp),
                colors = CardDefaults.elevatedCardColors(
                    containerColor = MaterialTheme.colorScheme.surfaceContainer,
                ),
            ) {
                Column(
                    modifier = Modifier.padding(16.dp),
                    verticalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    Text("基本配置", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
                    OutlinedTextField(
                        value = name, onValueChange = { name = it },
                        label = { Text("名称") },
                        modifier = Modifier.fillMaxWidth(),
                        shape = RoundedCornerShape(12.dp),
                        singleLine = true,
                    )
                    var expanded by remember { mutableStateOf(false) }
                    ExposedDropdownMenuBox(
                        expanded = expanded,
                        onExpandedChange = { expanded = it }
                    ) {
                        OutlinedTextField(
                            value = type,
                            onValueChange = {},
                            readOnly = true,
                            label = { Text("类型") },
                            modifier = Modifier.menuAnchor().fillMaxWidth(),
                            shape = RoundedCornerShape(12.dp),
                            singleLine = true,
                        )
                        ExposedDropdownMenu(expanded = expanded, onDismissRequest = { expanded = false }) {
                            listOf("ManualFire", "OnConnect", "OnReconnect", "OnIpChange").forEach {
                                DropdownMenuItem(
                                    text = { Text(it) },
                                    onClick = { type = it; expanded = false }
                                )
                            }
                        }
                    }
                    OutlinedTextField(
                        value = timeout, onValueChange = { timeout = it },
                        label = { Text("超时 (秒)") },
                        modifier = Modifier.fillMaxWidth(),
                        shape = RoundedCornerShape(12.dp),
                        singleLine = true,
                    )
                    OutlinedTextField(
                        value = commands,
                        onValueChange = { commands = it },
                        label = { Text("命令 (每行一个)") },
                        modifier = Modifier.fillMaxWidth().height(160.dp),
                        shape = RoundedCornerShape(12.dp),
                    )
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.SpaceBetween,
                    ) {
                        Text("启用", style = MaterialTheme.typography.bodyLarge)
                        Switch(checked = enabled, onCheckedChange = { enabled = it })
                    }
                }
            }

            ElevatedCard(
                modifier = Modifier.fillMaxWidth(),
                shape = RoundedCornerShape(16.dp),
                colors = CardDefaults.elevatedCardColors(
                    containerColor = MaterialTheme.colorScheme.surfaceContainer,
                ),
            ) {
                Column(
                    modifier = Modifier.padding(16.dp),
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    Text("通知", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.SpaceBetween,
                    ) {
                        Text("成功时发通知", style = MaterialTheme.typography.bodyLarge)
                        Switch(checked = notifyOnSuccess, onCheckedChange = { notifyOnSuccess = it })
                    }
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.SpaceBetween,
                    ) {
                        Text("失败时发通知", style = MaterialTheme.typography.bodyLarge)
                        Switch(checked = notifyOnFailure, onCheckedChange = { notifyOnFailure = it })
                    }
                }
            }

            Button(
                onClick = {
                    val list = repo.listTriggers(serverId).toMutableList()
                    val id = if (triggerId.isNullOrEmpty()) UUID.randomUUID().toString() else triggerId
                    val t = TriggerInstance(
                        id = id,
                        name = name,
                        trigger_type = type,
                        enabled = enabled,
                        timeout_secs = timeout.toLongOrNull() ?: 30,
                        notify_on_success = notifyOnSuccess,
                        notify_on_failure = notifyOnFailure,
                        commands = commands.lines().map { it.trim() }.filter { it.isNotEmpty() },
                    )
                    if (list.any { it.id == id }) {
                        list.replaceAll { if (it.id == id) t else it }
                    } else {
                        list.add(t)
                    }
                    repo.setServerTriggers(serverId, list)
                    navController.popBackStack()
                },
                modifier = Modifier.fillMaxWidth().height(48.dp),
                shape = RoundedCornerShape(14.dp),
            ) {
                Text("保存", fontWeight = FontWeight.SemiBold)
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun TemplateLibraryDialog(onDismiss: () -> Unit) {
    val repo = remember { RustRepository }
    var templates by remember { mutableStateOf(repo.listTriggerTemplates()) }
    var editingTemplate by remember { mutableStateOf<TriggerTemplate?>(null) }
    var showNewDialog by remember { mutableStateOf(false) }

    if (showNewDialog) {
        TemplateEditDialog(
            template = TriggerTemplate(id = UUID.randomUUID().toString()),
            onDismiss = { showNewDialog = false },
            onSave = { saved ->
                val updated = templates.filter { it.id != saved.id } + saved
                repo.setTriggerTemplates(updated)
                templates = repo.listTriggerTemplates()
                showNewDialog = false
            },
        )
    }

    editingTemplate?.let { tpl ->
        TemplateEditDialog(
            template = tpl,
            onDismiss = { editingTemplate = null },
            onSave = { saved ->
                val updated = templates.filter { it.id != saved.id } + saved
                repo.setTriggerTemplates(updated)
                templates = repo.listTriggerTemplates()
                editingTemplate = null
            },
        )
    }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("模板库", fontWeight = FontWeight.SemiBold) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                templates.forEach { tpl ->
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Column(modifier = Modifier.weight(1f)) {
                            Text(tpl.name, style = MaterialTheme.typography.bodyLarge)
                            if (tpl.description.isNotBlank()) {
                                Text(
                                    tpl.description,
                                    style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                )
                            }
                            if (tpl.built_in) {
                                Text(
                                    "内置",
                                    style = MaterialTheme.typography.labelSmall,
                                    color = MaterialTheme.colorScheme.primary,
                                )
                            }
                        }
                        if (!tpl.built_in) {
                            IconButton(onClick = { editingTemplate = tpl }) {
                                Icon(Icons.Filled.Edit, contentDescription = "编辑")
                            }
                            IconButton(onClick = {
                                val updated = templates.filter { it.id != tpl.id }
                                repo.setTriggerTemplates(updated)
                                templates = repo.listTriggerTemplates()
                            }) {
                                Icon(Icons.Filled.Delete, contentDescription = "删除", tint = MaterialTheme.colorScheme.error)
                            }
                        }
                    }
                    HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.3f))
                }
                if (templates.isEmpty()) {
                    Text("暂无模板", color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
        },
        confirmButton = {
            Button(onClick = { showNewDialog = true }) {
                Icon(Icons.Filled.Add, contentDescription = null)
                Text("新建模板")
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("关闭") } },
    )
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun TemplateEditDialog(
    template: TriggerTemplate,
    onDismiss: () -> Unit,
    onSave: (TriggerTemplate) -> Unit,
) {
    var name by remember { mutableStateOf(template.name) }
    var description by remember { mutableStateOf(template.description) }
    var type by remember { mutableStateOf(template.trigger_type) }
    var commands by remember { mutableStateOf(template.commands.joinToString("\n")) }
    var timeout by remember { mutableStateOf(template.timeout_secs.toString()) }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("编辑模板", fontWeight = FontWeight.SemiBold) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedTextField(
                    value = name, onValueChange = { name = it },
                    label = { Text("名称") },
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                )
                OutlinedTextField(
                    value = description, onValueChange = { description = it },
                    label = { Text("描述") },
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                )
                var expanded by remember { mutableStateOf(false) }
                ExposedDropdownMenuBox(
                    expanded = expanded,
                    onExpandedChange = { expanded = it }
                ) {
                    OutlinedTextField(
                        value = type,
                        onValueChange = {},
                        readOnly = true,
                        label = { Text("类型") },
                        modifier = Modifier.menuAnchor().fillMaxWidth(),
                        singleLine = true,
                    )
                    ExposedDropdownMenu(expanded = expanded, onDismissRequest = { expanded = false }) {
                        listOf("ManualFire", "OnConnect", "OnReconnect", "OnIpChange").forEach {
                            DropdownMenuItem(
                                text = { Text(it) },
                                onClick = { type = it; expanded = false }
                            )
                        }
                    }
                }
                OutlinedTextField(
                    value = timeout, onValueChange = { timeout = it },
                    label = { Text("超时 (秒)") },
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                )
                OutlinedTextField(
                    value = commands, onValueChange = { commands = it },
                    label = { Text("命令 (每行一个)") },
                    modifier = Modifier.fillMaxWidth().height(120.dp),
                )
            }
        },
        confirmButton = {
            Button(
                onClick = {
                    onSave(template.copy(
                        name = name,
                        description = description,
                        trigger_type = type,
                        commands = commands.lines().map { it.trim() }.filter { it.isNotEmpty() },
                        timeout_secs = timeout.toLongOrNull() ?: 30,
                        built_in = false,
                    ))
                },
                enabled = name.isNotBlank(),
            ) { Text("保存") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("取消") } },
    )
}
