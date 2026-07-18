package com.termfast.app.ui.screen

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material.icons.filled.Apps
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.Notifications
import androidx.compose.material.icons.filled.VpnKey
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.navigation.NavController
import com.termfast.app.data.AppSettings
import com.termfast.app.data.CredentialManager
import com.termfast.app.data.SettingsRepository
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(navController: NavController) {
    val context = LocalContext.current
    val repo = remember { SettingsRepository(context) }
    var settings by remember { mutableStateOf(repo.load()) }

    fun update(block: AppSettings.() -> AppSettings) {
        val s = settings.block()
        settings = s
        repo.save(s)
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("设置", fontWeight = FontWeight.SemiBold) },
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = MaterialTheme.colorScheme.surface,
                ),
            )
        }
    ) { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 16.dp, vertical = 12.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            // VPN section
            SettingsSectionCard(title = "VPN", icon = Icons.Filled.VpnKey) {
                OutlinedTextField(
                    value = settings.vpnMtu.toString(),
                    onValueChange = { update { copy(vpnMtu = it.toIntOrNull() ?: 1400) } },
                    label = { Text("MTU") },
                    modifier = Modifier.fillMaxWidth(),
                    shape = RoundedCornerShape(12.dp),
                    singleLine = true,
                )
                Spacer(Modifier.height(8.dp))
                SwitchRow(label = "IPv6 路由", checked = settings.ipv6Enabled, onCheckedChange = { update { copy(ipv6Enabled = it) } })
                SwitchRow(label = "路由 ULA (fc00::/7)", checked = settings.routeUla, onCheckedChange = { update { copy(routeUla = it) } })
                SwitchRow(label = "Kill Switch", checked = settings.killSwitchEnabled, onCheckedChange = { update { copy(killSwitchEnabled = it) } })
                var dnsExpanded by remember { mutableStateOf(false) }
                ExposedDropdownMenuBox(
                    expanded = dnsExpanded,
                    onExpandedChange = { dnsExpanded = it }
                ) {
                    OutlinedTextField(
                        value = settings.dnsStrategy,
                        onValueChange = {},
                        readOnly = true,
                        label = { Text("DNS 策略") },
                        modifier = Modifier.menuAnchor().fillMaxWidth(),
                        shape = RoundedCornerShape(12.dp),
                        singleLine = true,
                    )
                    ExposedDropdownMenu(expanded = dnsExpanded, onDismissRequest = { dnsExpanded = false }) {
                        listOf("over-tcp", "over-udp", "none").forEach { strategy ->
                            DropdownMenuItem(
                                text = { Text(strategy) },
                                onClick = {
                                    update { copy(dnsStrategy = strategy) }
                                    dnsExpanded = false
                                }
                            )
                        }
                    }
                }
            }

            // Per-app proxy
            SettingsNavCard(
                icon = Icons.Filled.Apps,
                title = "分应用代理",
                subtitle = "配置哪些 App 走代理",
                onClick = { navController.navigate("per_app_proxy") },
            )

            // Credential security section
            CredentialSection()

            // Notifications section
            SettingsSectionCard(title = "通知", icon = Icons.Filled.Notifications) {
                NotificationSwitch("连接成功", settings.notify_connect_success) { update { copy(notify_connect_success = it) } }
                NotificationSwitch("断开连接", settings.notify_disconnect) { update { copy(notify_disconnect = it) } }
                NotificationSwitch("认证失败", settings.notify_auth_fail) { update { copy(notify_auth_fail = it) } }
                NotificationSwitch("代理状态变化", settings.notify_proxy_toggle) { update { copy(notify_proxy_toggle = it) } }
                NotificationSwitch("触发器成功", settings.notify_trigger_success) { update { copy(notify_trigger_success = it) } }
                NotificationSwitch("触发器失败", settings.notify_trigger_fail) { update { copy(notify_trigger_fail = it) } }
                NotificationSwitch("IP 变化", settings.notify_ip_change) { update { copy(notify_ip_change = it) } }
            }

            // About section
            SettingsSectionCard(title = "关于", icon = Icons.Filled.Info) {
                InfoRow(label = "版本", value = "0.1.9")
                HorizontalDivider(modifier = Modifier.padding(vertical = 4.dp), color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.3f))
                InfoRow(label = "开源协议", value = "Apache-2.0")
            }
        }
    }
}

// === SECTION 1 END ===

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun CredentialSection() {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var credStatus by remember { mutableStateOf(CredentialManager.status()) }
    var showSetup by remember { mutableStateOf(false) }
    var showChangePw by remember { mutableStateOf(false) }
    var showReset by remember { mutableStateOf(false) }
    var showImportPw by remember { mutableStateOf(false) }
    var pendingImportPath by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    var msg by remember { mutableStateOf<String?>(null) }

    fun refreshStatus() { credStatus = CredentialManager.status() }

    val isPending = credStatus == CredentialManager.Status.PENDING

    // File picker for export (create document).
    val exportLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.CreateDocument("application/octet-stream")
    ) { uri ->
        if (uri == null) return@rememberLauncherForActivityResult
        scope.launch {
            val ok = withContext(Dispatchers.IO) {
                // Export to temp file, then copy to the chosen URI.
                val tmp = java.io.File(context.cacheDir, "cred_export_tmp.enc")
                if (!CredentialManager.export(tmp.absolutePath)) return@withContext false
                context.contentResolver.openOutputStream(uri)?.use { out ->
                    tmp.inputStream().use { it.copyTo(out) }
                } ?: return@withContext false
                tmp.delete()
                true
            }
            msg = if (ok) "导出成功" else "导出失败"
        }
    }

    // File picker for import (open document).
    val importLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.OpenDocument()
    ) { uri ->
        if (uri == null) return@rememberLauncherForActivityResult
        // Copy to temp file first, then prompt for password.
        scope.launch {
            val tmpPath = withContext(Dispatchers.IO) {
                val tmp = java.io.File(context.cacheDir, "cred_import_tmp.enc")
                context.contentResolver.openInputStream(uri)?.use { input ->
                    tmp.outputStream().use { input.copyTo(it) }
                } ?: return@withContext null
                tmp.absolutePath
            }
            if (tmpPath != null) {
                pendingImportPath = tmpPath
                showImportPw = true
            } else {
                msg = "读取文件失败"
            }
        }
    }

    SettingsSectionCard(title = "凭据安全", icon = Icons.Filled.VpnKey) {
        if (isPending) {
            // No password set yet — show setup button.
            Text(
                "凭据尚未加密。设置主密码后，保存的凭据将受到 AES-256 加密保护。",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(8.dp))
            Button(
                onClick = { showSetup = true },
                modifier = Modifier.fillMaxWidth(),
            ) { Text("设置主密码") }
        } else {
            // Change master password
            ButtonRow(
                label = "修改主密码",
                onClick = { showChangePw = true },
            )
            HorizontalDivider(modifier = Modifier.padding(vertical = 4.dp), color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.3f))
            // Export
            ButtonRow(
                label = "导出加密备份",
                onClick = { exportLauncher.launch("termfast-credentials.enc") },
            )
            HorizontalDivider(modifier = Modifier.padding(vertical = 4.dp), color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.3f))
            // Import
            ButtonRow(
                label = "导入加密备份",
                onClick = { importLauncher.launch(arrayOf("*/*")) },
            )
            HorizontalDivider(modifier = Modifier.padding(vertical = 4.dp), color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.3f))
            // Reset (dangerous)
            ButtonRow(
                label = "忘记密码",
                isDanger = true,
                onClick = { showReset = true },
            )
        }
        msg?.let {
            Spacer(Modifier.height(8.dp))
            Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.primary)
        }
    }

    // Setup password dialog
    if (showSetup) {
        SetupPasswordDialog(
            busy = busy,
            onConfirm = { pw ->
                busy = true
                scope.launch {
                    val ok = withContext(Dispatchers.IO) {
                        CredentialManager.initialize(context, pw)
                    }
                    busy = false
                    if (ok) { showSetup = false; msg = "主密码已设置"; refreshStatus() }
                    else msg = "设置失败，请重试"
                }
            },
            onDismiss = { showSetup = false },
        )
    }

    // Change password dialog
    if (showChangePw) {
        ChangePasswordDialog(
            busy = busy,
            onConfirm = { old, new ->
                busy = true
                scope.launch {
                    val ok = withContext(Dispatchers.IO) {
                        CredentialManager.changePassword(context, old, new)
                    }
                    busy = false
                    if (ok) { showChangePw = false; msg = "主密码已修改" }
                    else msg = "修改失败，旧密码可能不正确"
                }
            },
            onDismiss = { showChangePw = false },
        )
    }

    // Reset confirm dialog
    if (showReset) {
        AlertDialog(
            onDismissRequest = { showReset = false },
            title = { Text("忘记密码") },
            text = { Text("将删除加密凭据文件和当前主密码。所有已保存的 SSH 密码和私钥将永久丢失，无法恢复。\n\n服务器列表不受影响，但每个服务器需要重新输入密码或密钥才能连接。\n\n如果之前导出过加密备份，可以通过「导入加密备份」恢复。") },
            confirmButton = {
                TextButton(
                    onClick = {
                        busy = true
                        scope.launch {
                            val ok = withContext(Dispatchers.IO) {
                                CredentialManager.reset(context)
                            }
                            busy = false
                            showReset = false
                            msg = if (ok) "已清除所有凭据" else "操作失败"
                            refreshStatus()
                        }
                    },
                    enabled = !busy,
                ) { Text("确认清除", color = MaterialTheme.colorScheme.error) }
            },
            dismissButton = { TextButton(onClick = { showReset = false }) { Text("取消") } },
        )
    }

    // Import password dialog
    var importError by remember { mutableStateOf<String?>(null) }
    if (showImportPw && pendingImportPath != null) {
        ImportPasswordDialog(
            busy = busy,
            error = importError,
            onConfirm = { pw ->
                val path = pendingImportPath!!
                busy = true
                importError = null
                scope.launch {
                    val ok = withContext(Dispatchers.IO) {
                        CredentialManager.import(path, pw)
                    }
                    busy = false
                    if (ok) {
                        showImportPw = false
                        pendingImportPath = null
                        java.io.File(path).delete()
                        msg = "导入成功"
                        refreshStatus()
                    } else {
                        // Keep dialog open, show error, let user retry.
                        importError = "密码错误或文件损坏"
                    }
                }
            },
            onDismiss = {
                showImportPw = false
                pendingImportPath?.let { java.io.File(it).delete() }
                pendingImportPath = null
                importError = null
            },
        )
    }
}

@Composable
private fun ButtonRow(label: String, isDanger: Boolean = false, onClick: () -> Unit) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable { onClick() }
            .padding(vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            label,
            style = MaterialTheme.typography.bodyLarge,
            color = if (isDanger) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurface,
            modifier = Modifier.weight(1f),
        )
        Icon(
            Icons.AutoMirrored.Filled.KeyboardArrowRight,
            contentDescription = null,
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun ImportPasswordDialog(
    busy: Boolean,
    error: String?,
    onConfirm: (String) -> Unit,
    onDismiss: () -> Unit,
) {
    var pw by remember { mutableStateOf("") }

    AlertDialog(
        onDismissRequest = { onDismiss() },
        title = { Text("导入加密备份") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(
                    "请输入该备份文件的主密码。密码验证通过后才会覆盖当前凭据。",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                OutlinedTextField(
                    value = pw,
                    onValueChange = { pw = it },
                    label = { Text("主密码") },
                    visualTransformation = PasswordVisualTransformation(),
                    singleLine = true,
                    isError = error != null,
                )
                if (error != null) {
                    Text(
                        error,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.error,
                    )
                }
            }
        },
        confirmButton = {
            TextButton(
                onClick = { onConfirm(pw) },
                enabled = pw.isNotEmpty() && !busy,
            ) { Text(if (busy) "验证中..." else "导入") }
        },
        dismissButton = { TextButton(onClick = { onDismiss() }) { Text("取消") } },
    )
}

@Composable
private fun SetupPasswordDialog(
    busy: Boolean,
    onConfirm: (String) -> Unit,
    onDismiss: () -> Unit,
) {
    var pw by remember { mutableStateOf("") }
    var confirm by remember { mutableStateOf("") }
    val canSubmit = pw.length >= 4 && pw == confirm && !busy

    AlertDialog(
        onDismissRequest = { onDismiss() },
        title = { Text("设置主密码") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(
                    "主密码用于加密你的 SSH 凭据。请妥善保管，丢失后需要重置所有凭据。",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                OutlinedTextField(
                    value = pw,
                    onValueChange = { pw = it },
                    label = { Text("主密码") },
                    visualTransformation = PasswordVisualTransformation(),
                    singleLine = true,
                )
                OutlinedTextField(
                    value = confirm,
                    onValueChange = { confirm = it },
                    label = { Text("确认主密码") },
                    visualTransformation = PasswordVisualTransformation(),
                    singleLine = true,
                    isError = confirm.isNotEmpty() && pw != confirm,
                )
            }
        },
        confirmButton = {
            TextButton(
                onClick = { onConfirm(pw) },
                enabled = canSubmit,
            ) { Text(if (busy) "处理中..." else "设置") }
        },
        dismissButton = { TextButton(onClick = { onDismiss() }) { Text("取消") } },
    )
}

@Composable
private fun ChangePasswordDialog(
    busy: Boolean,
    onConfirm: (String, String) -> Unit,
    onDismiss: () -> Unit,
) {
    var old by remember { mutableStateOf("") }
    var new by remember { mutableStateOf("") }
    var confirm by remember { mutableStateOf("") }
    val canSubmit = old.isNotEmpty() && new.length >= 4 && new == confirm && !busy

    AlertDialog(
        onDismissRequest = { onDismiss() },
        title = { Text("修改主密码") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedTextField(
                    value = old,
                    onValueChange = { old = it },
                    label = { Text("当前主密码") },
                    visualTransformation = PasswordVisualTransformation(),
                    singleLine = true,
                )
                OutlinedTextField(
                    value = new,
                    onValueChange = { new = it },
                    label = { Text("新主密码") },
                    visualTransformation = PasswordVisualTransformation(),
                    singleLine = true,
                )
                OutlinedTextField(
                    value = confirm,
                    onValueChange = { confirm = it },
                    label = { Text("确认新密码") },
                    visualTransformation = PasswordVisualTransformation(),
                    singleLine = true,
                    isError = confirm.isNotEmpty() && new != confirm,
                )
            }
        },
        confirmButton = {
            TextButton(
                onClick = { onConfirm(old, new) },
                enabled = canSubmit,
            ) { Text(if (busy) "处理中..." else "修改") }
        },
        dismissButton = { TextButton(onClick = { onDismiss() }) { Text("取消") } },
    )
}

@Composable
private fun SettingsSectionCard(
    title: String,
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    content: @Composable ColumnScope.() -> Unit,
) {
    ElevatedCard(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(16.dp),
        colors = CardDefaults.elevatedCardColors(
            containerColor = MaterialTheme.colorScheme.surfaceContainer,
        ),
    ) {
        Column(modifier = Modifier.padding(16.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(
                    icon,
                    contentDescription = null,
                    modifier = Modifier.size(20.dp),
                    tint = MaterialTheme.colorScheme.primary,
                )
                Spacer(Modifier.width(8.dp))
                Text(title, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
            }
            Spacer(Modifier.height(12.dp))
            content()
        }
    }
}

@Composable
private fun SettingsNavCard(
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    title: String,
    subtitle: String,
    onClick: () -> Unit,
) {
    ElevatedCard(
        onClick = onClick,
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(16.dp),
        colors = CardDefaults.elevatedCardColors(
            containerColor = MaterialTheme.colorScheme.surfaceContainer,
        ),
    ) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(16.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(
                icon,
                contentDescription = null,
                modifier = Modifier.size(24.dp),
                tint = MaterialTheme.colorScheme.primary,
            )
            Spacer(Modifier.width(12.dp))
            Column(modifier = Modifier.weight(1f)) {
                Text(title, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.Medium)
                Text(subtitle, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            Icon(
                Icons.AutoMirrored.Filled.KeyboardArrowRight,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun SwitchRow(label: String, checked: Boolean, onCheckedChange: (Boolean) -> Unit) {
    Row(
        modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        Text(label, style = MaterialTheme.typography.bodyLarge)
        Switch(checked = checked, onCheckedChange = onCheckedChange)
    }
}

@Composable
private fun NotificationSwitch(label: String, checked: Boolean, onCheckedChange: (Boolean) -> Unit) {
    SwitchRow(label = label, checked = checked, onCheckedChange = onCheckedChange)
}

@Composable
private fun InfoRow(label: String, value: String) {
    Row(
        modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label, style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Text(value, style = MaterialTheme.typography.bodyLarge, fontWeight = FontWeight.Medium)
    }
}