package com.termfast.app.ui.screen

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Stop
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import com.termfast.app.R
import com.termfast.app.RustBridge
import com.termfast.app.data.PortForwardRule
import com.termfast.app.data.PortForwardRuleWithStatus
import com.termfast.app.data.PortForwardListResponse
import com.termfast.app.data.PortForwardAddResponse
import com.termfast.app.data.PortForwardOpResponse
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json

private val json = Json { ignoreUnknownKeys = true; encodeDefaults = true }

// Quick templates for common services
private data class QuickTemplate(val name: String, val localPort: Int, val remotePort: Int)

private val QUICK_TEMPLATES = listOf(
    QuickTemplate("MySQL", 13306, 3306),
    QuickTemplate("Redis", 16379, 6379),
    QuickTemplate("PostgreSQL", 15432, 5432),
    QuickTemplate("Web", 18080, 8080),
)

// === SECTION 1 END ===

@Composable
fun PortForwardTab(serverId: String) {
    val scope = rememberCoroutineScope()
    var rules by remember { mutableStateOf<List<PortForwardRuleWithStatus>>(emptyList()) }
    var loading by remember { mutableStateOf(true) }
    var showEditor by remember { mutableStateOf(false) }
    var editingRule by remember { mutableStateOf<PortForwardRule?>(null) }
    var deletingRule by remember { mutableStateOf<PortForwardRuleWithStatus?>(null) }
    var togglingId by remember { mutableStateOf<String?>(null) }
    var errorMsg by remember { mutableStateOf<String?>(null) }

    fun loadRules() {
        scope.launch {
            loading = true
            try {
                val result = withContext(Dispatchers.IO) {
                    RustBridge.nativeListPortForwards(serverId)
                }
                val resp = json.decodeFromString<PortForwardListResponse>(result)
                rules = resp.rules
            } catch (e: Exception) {
                errorMsg = e.message
            } finally {
                loading = false
            }
        }
    }

    LaunchedEffect(serverId) { loadRules() }

    fun startRule(ruleId: String) {
        scope.launch {
            togglingId = ruleId
            try {
                val result = withContext(Dispatchers.IO) {
                    RustBridge.nativeStartPortForward(serverId, ruleId)
                }
                val resp = json.decodeFromString<PortForwardOpResponse>(result)
                if (resp.error != null) errorMsg = resp.error
            } catch (e: Exception) {
                errorMsg = e.message
            } finally {
                togglingId = null
                loadRules()
            }
        }
    }

    fun stopRule(ruleId: String) {
        scope.launch {
            togglingId = ruleId
            try {
                val result = withContext(Dispatchers.IO) {
                    RustBridge.nativeStopPortForward(serverId, ruleId)
                }
                val resp = json.decodeFromString<PortForwardOpResponse>(result)
                if (resp.error != null) errorMsg = resp.error
            } catch (e: Exception) {
                errorMsg = e.message
            } finally {
                togglingId = null
                loadRules()
            }
        }
    }

    fun deleteRule(ruleId: String) {
        scope.launch {
            try {
                withContext(Dispatchers.IO) {
                    RustBridge.nativeDeletePortForward(serverId, ruleId)
                }
                deletingRule = null
                loadRules()
            } catch (e: Exception) {
                errorMsg = e.message
            }
        }
    }

    // === SECTION 2 END ===

    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(16.dp)
    ) {
        // Error banner
        errorMsg?.let { msg ->
            Card(
                colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.errorContainer),
                modifier = Modifier.fillMaxWidth().padding(bottom = 12.dp)
            ) {
                Text(
                    text = msg,
                    modifier = Modifier.padding(12.dp),
                    color = MaterialTheme.colorScheme.onErrorContainer,
                    style = MaterialTheme.typography.bodySmall
                )
            }
        }

        // Quick templates
        Text(
            text = stringResource(R.string.pf_quick_templates),
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(bottom = 8.dp)
        )
        // Quick templates — 3 per row
        QUICK_TEMPLATES.chunked(3).forEach { row ->
            Row(
                modifier = Modifier.fillMaxWidth().padding(bottom = 8.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp)
            ) {
                row.forEach { tmpl ->
                    AssistChip(
                        onClick = {
                            editingRule = PortForwardRule(
                                name = tmpl.name,
                                type = "local",
                                local_host = "127.0.0.1",
                                local_port = tmpl.localPort,
                                remote_host = "127.0.0.1",
                                remote_port = tmpl.remotePort,
                            )
                            showEditor = true
                        },
                        label = { Text("+ ${tmpl.name}", maxLines = 1) },
                        modifier = Modifier.weight(1f)
                    )
                }
                // Fill empty slots to keep alignment
                repeat(3 - row.size) {
                    Spacer(Modifier.weight(1f))
                }
            }
        }
        Spacer(Modifier.height(8.dp))

        // Add button
        Button(
            onClick = {
                editingRule = null
                showEditor = true
            },
            modifier = Modifier.fillMaxWidth().padding(bottom = 16.dp)
        ) {
            Icon(Icons.Default.Add, contentDescription = null, modifier = Modifier.size(18.dp))
            Spacer(Modifier.width(8.dp))
            Text(stringResource(R.string.pf_add_rule))
        }

        // Rules list
        if (loading) {
            Box(
                modifier = Modifier.fillMaxWidth().padding(32.dp),
                contentAlignment = Alignment.Center
            ) {
                CircularProgressIndicator()
            }
        } else if (rules.isEmpty()) {
            Text(
                text = stringResource(R.string.pf_empty),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.fillMaxWidth().padding(32.dp),
                textAlign = androidx.compose.ui.text.style.TextAlign.Center
            )
        } else {
            rules.forEach { rule ->
                PortForwardRuleCard(
                    rule = rule,
                    toggling = togglingId == rule.id,
                    onStart = { startRule(rule.id) },
                    onStop = { stopRule(rule.id) },
                    onEdit = {
                        editingRule = rule.let {
                            PortForwardRule(
                                id = it.id, name = it.name, type = it.type,
                                local_host = it.local_host, local_port = it.local_port,
                                remote_host = it.remote_host, remote_port = it.remote_port,
                                enabled = it.enabled, auto_start = it.auto_start,
                            )
                        }
                        showEditor = true
                    },
                    onDelete = { deletingRule = rule }
                )
                Spacer(Modifier.height(8.dp))
            }
        }
    }

    // Editor dialog
    if (showEditor) {
        PortForwardEditorDialog(
            serverId = serverId,
            rule = editingRule,
            onDismiss = { showEditor = false; editingRule = null },
            onSaved = { showEditor = false; editingRule = null; loadRules() }
        )
    }

    // Delete confirmation
    deletingRule?.let { rule ->
        AlertDialog(
            onDismissRequest = { deletingRule = null },
            title = { Text(stringResource(R.string.pf_delete_title)) },
            text = {
                Text(
                    if (rule.running)
                        stringResource(R.string.pf_delete_confirm_running, rule.name)
                    else
                        stringResource(R.string.pf_delete_confirm, rule.name)
                )
            },
            confirmButton = {
                TextButton(onClick = { deleteRule(rule.id) }) { Text(stringResource(R.string.pf_delete)) }
            },
            dismissButton = {
                TextButton(onClick = { deletingRule = null }) { Text(stringResource(R.string.pf_cancel)) }
            }
        )
    }
}

// === SECTION 3 END ===

@Composable
private fun PortForwardRuleCard(
    rule: PortForwardRuleWithStatus,
    toggling: Boolean,
    onStart: () -> Unit,
    onStop: () -> Unit,
    onEdit: () -> Unit,
    onDelete: () -> Unit,
) {
    Card(
        modifier = Modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant),
        shape = RoundedCornerShape(12.dp)
    ) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(12.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            // Status dot
            val dotColor = when {
                rule.error != null -> MaterialTheme.colorScheme.error
                rule.running -> MaterialTheme.colorScheme.primary
                rule.enabled -> MaterialTheme.colorScheme.outline
                else -> MaterialTheme.colorScheme.surface
            }
            Box(
                modifier = Modifier
                    .size(8.dp)
                    .clip(RoundedCornerShape(4.dp))
                    .background(dotColor)
            )
            Spacer(Modifier.width(12.dp))

            // Rule info
            Column(modifier = Modifier.weight(1f)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        text = rule.name,
                        style = MaterialTheme.typography.bodyMedium,
                        fontWeight = FontWeight.Medium
                    )
                    Spacer(Modifier.width(8.dp))
                    Text(
                        text = if (rule.type == "local") "-L" else "-R",
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        fontFamily = FontFamily.Monospace
                    )
                }
                Text(
                    text = "${rule.local_host}:${rule.local_port} → ${rule.remote_host}:${rule.remote_port}",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    fontFamily = FontFamily.Monospace
                )
                if (rule.active_connections > 0 || rule.bytes_in > 0 || rule.bytes_out > 0 || rule.error != null) {
                    val parts = mutableListOf<String>()
                    val connStr = stringResource(R.string.pf_connections)
                    if (rule.active_connections > 0) parts.add("${rule.active_connections} $connStr")
                    if (rule.bytes_in > 0 || rule.bytes_out > 0) {
                        parts.add("↓${formatBytes(rule.bytes_in)} ↑${formatBytes(rule.bytes_out)}")
                    }
                    if (rule.error != null) parts.add(stringResource(R.string.pf_error))
                    Text(
                        text = parts.joinToString(" · "),
                        style = MaterialTheme.typography.labelSmall,
                        color = if (rule.error != null) MaterialTheme.colorScheme.error
                               else MaterialTheme.colorScheme.onSurfaceVariant
                    )
                }
            }

            // Action buttons
            if (rule.running) {
                IconButton(onClick = onStop, enabled = !toggling) {
                    Icon(Icons.Default.Stop, contentDescription = stringResource(R.string.pf_stop))
                }
            } else {
                IconButton(onClick = onStart, enabled = !toggling && rule.enabled) {
                    Icon(Icons.Default.PlayArrow, contentDescription = stringResource(R.string.pf_start))
                }
            }
            IconButton(onClick = onEdit) {
                Icon(Icons.Default.Edit, contentDescription = stringResource(R.string.pf_edit))
            }
            IconButton(onClick = onDelete) {
                Icon(Icons.Default.Delete, contentDescription = stringResource(R.string.pf_delete))
            }
        }
    }
}

private fun formatBytes(bytes: Long): String {
    if (bytes < 1024) return "${bytes}B"
    if (bytes < 1024 * 1024) return "${"%.1f".format(bytes / 1024.0)}K"
    if (bytes < 1024 * 1024 * 1024) return "${"%.1f".format(bytes / (1024.0 * 1024))}M"
    return "${"%.1f".format(bytes / (1024.0 * 1024 * 1024))}G"
}

// === SECTION 4 END ===

@Composable
private fun PortForwardEditorDialog(
    serverId: String,
    rule: PortForwardRule?,
    onDismiss: () -> Unit,
    onSaved: () -> Unit,
) {
    val scope = rememberCoroutineScope()
    val context = androidx.compose.ui.platform.LocalContext.current
    var name by remember { mutableStateOf(rule?.name ?: "") }
    var type by remember { mutableStateOf(rule?.type ?: "local") }
    var localHost by remember { mutableStateOf(rule?.local_host ?: "127.0.0.1") }
    var localPort by remember { mutableStateOf((rule?.local_port ?: 1080).toString()) }
    var remoteHost by remember { mutableStateOf(rule?.remote_host ?: "127.0.0.1") }
    var remotePort by remember { mutableStateOf((rule?.remote_port ?: 80).toString()) }
    var enabled by remember { mutableStateOf(rule?.enabled ?: true) }
    var autoStart by remember { mutableStateOf(rule?.auto_start ?: false) }
    var saving by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }

    val isEdit = rule != null && rule.id.isNotEmpty()

    AlertDialog(
        onDismissRequest = { if (!saving) onDismiss() },
        title = { Text(stringResource(if (isEdit) R.string.pf_edit_rule else R.string.pf_add_rule)) },
        text = {
            Column(
                modifier = Modifier.fillMaxWidth().verticalScroll(rememberScrollState()),
                verticalArrangement = Arrangement.spacedBy(12.dp)
            ) {
                error?.let {
                    Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall)
                }

                OutlinedTextField(
                    value = name,
                    onValueChange = { name = it },
                    label = { Text(stringResource(R.string.pf_name)) },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth()
                )

                // Type selector
                Text(stringResource(R.string.pf_type), style = MaterialTheme.typography.labelMedium)
                Row(modifier = Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    FilterChip(
                        selected = type == "local",
                        onClick = { type = "local" },
                        label = { Text(stringResource(R.string.pf_local)) },
                        modifier = Modifier.weight(1f)
                    )
                    FilterChip(
                        selected = type == "remote",
                        onClick = { type = "remote" },
                        label = { Text(stringResource(R.string.pf_remote)) },
                        modifier = Modifier.weight(1f)
                    )
                }

                Row(modifier = Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    OutlinedTextField(
                        value = localHost,
                        onValueChange = { localHost = it },
                        label = { Text(stringResource(if (type == "local") R.string.pf_local_host else R.string.pf_remote_bind_host)) },
                        singleLine = true,
                        modifier = Modifier.weight(1.5f)
                    )
                    OutlinedTextField(
                        value = localPort,
                        onValueChange = { localPort = it.filter { c -> c.isDigit() } },
                        label = { Text(stringResource(if (type == "local") R.string.pf_local_port else R.string.pf_remote_bind_port)) },
                        singleLine = true,
                        modifier = Modifier.weight(1f)
                    )
                }

                Row(modifier = Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    OutlinedTextField(
                        value = remoteHost,
                        onValueChange = { remoteHost = it },
                        label = { Text(stringResource(if (type == "local") R.string.pf_remote_host else R.string.pf_local_target_host)) },
                        singleLine = true,
                        modifier = Modifier.weight(1.5f)
                    )
                    OutlinedTextField(
                        value = remotePort,
                        onValueChange = { remotePort = it.filter { c -> c.isDigit() } },
                        label = { Text(stringResource(if (type == "local") R.string.pf_remote_port else R.string.pf_local_target_port)) },
                        singleLine = true,
                        modifier = Modifier.weight(1f)
                    )
                }

                Row(verticalAlignment = Alignment.CenterVertically) {
                    Checkbox(checked = enabled, onCheckedChange = { enabled = it })
                    Text(stringResource(R.string.pf_enabled))
                }
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Checkbox(checked = autoStart, onCheckedChange = { autoStart = it })
                    Text(stringResource(R.string.pf_auto_start))
                }
            }
        },
        confirmButton = {
            TextButton(
                onClick = {
                    if (name.isBlank()) { error = context.getString(R.string.pf_name_required); return@TextButton }
                    val lp = localPort.toIntOrNull()
                    val rp = remotePort.toIntOrNull()
                    if (lp == null || lp < 1 || lp > 65535 || rp == null || rp < 1 || rp > 65535) {
                        error = context.getString(R.string.pf_invalid_port)
                        return@TextButton
                    }

                    saving = true
                    error = null
                    scope.launch {
                        try {
                            val ruleJson = json.encodeToString(
                                PortForwardRule.serializer(),
                                PortForwardRule(
                                    id = rule?.id ?: "",
                                    name = name.trim(),
                                    type = type,
                                    local_host = localHost,
                                    local_port = lp,
                                    remote_host = remoteHost,
                                    remote_port = rp,
                                    enabled = enabled,
                                    auto_start = autoStart,
                                )
                            )
                            val result = withContext(Dispatchers.IO) {
                                if (isEdit) {
                                    RustBridge.nativeUpdatePortForward(serverId, rule!!.id, ruleJson)
                                } else {
                                    RustBridge.nativeAddPortForward(serverId, ruleJson)
                                }
                            }
                            if (isEdit) {
                                val resp = json.decodeFromString<PortForwardOpResponse>(result)
                                if (resp.error != null) {
                                    error = resp.error
                                    saving = false
                                    return@launch
                                }
                            } else {
                                val resp = json.decodeFromString<PortForwardAddResponse>(result)
                                if (resp.error != null) {
                                    error = resp.error
                                    saving = false
                                    return@launch
                                }
                            }
                            onSaved()
                        } catch (e: Exception) {
                            error = e.message
                        } finally {
                            saving = false
                        }
                    }
                },
                enabled = !saving
            ) {
                Text(stringResource(if (saving) R.string.pf_saving else R.string.pf_save))
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss, enabled = !saving) { Text(stringResource(R.string.pf_cancel)) }
        }
    )
}

// === SECTION 5 END ===
