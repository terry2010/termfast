package com.termfast.app.ui.screen

import android.content.Intent
import android.net.Uri
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material.icons.filled.Cloud
import androidx.compose.material.icons.filled.CloudUpload
import androidx.compose.material.icons.filled.CloudDownload
import androidx.compose.material.icons.filled.LinkOff
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.ui.unit.dp
import com.termfast.app.data.CloudSyncManager
import com.termfast.app.data.CloudSyncManager.OAuthEvent
import com.termfast.app.data.CredentialManager
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * Cloud Sync section — shown in SettingsScreen below the Credential section.
 *
 * Supports Dropbox and Baidu Netdisk. Uses the encrypted cloud sync feature:
 * config is encrypted with the master password before upload, so the cloud
 * provider only sees ciphertext.
 *
 * OAuth flow uses a server-side relay callback (cloud-sync-callback.php)
 * that redirects to termfast://oauth/callback, caught by MainActivity's
 * deep-link intent filter.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun CloudSyncSection() {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var dropboxStatus by remember { mutableStateOf(CloudSyncManager.status(CloudSyncManager.Provider.DROPBOX)) }
    var baiduStatus by remember { mutableStateOf(CloudSyncManager.status(CloudSyncManager.Provider.BAIDU)) }
    var busy by remember { mutableStateOf(false) }
    var msg by remember { mutableStateOf<String?>(null) }
    // Upload dialog state: provider + whether this is a force upload (after conflict)
    var showUploadDialog by remember { mutableStateOf<Pair<String, Boolean>?>(null) }
    // Download dialog state: provider + whether this is a force download (after rollback)
    var showDownloadDialog by remember { mutableStateOf<Pair<String, Boolean>?>(null) }
    // Conflict dialog state: provider + reason
    var showConflictDialog by remember { mutableStateOf<Pair<String, String>?>(null) }
    // Rollback dialog state: provider + info map
    var showRollbackDialog by remember { mutableStateOf<Pair<String, Map<String, String?>>?>(null) }

    // Collect OAuth events (deep link callback)
    LaunchedEffect(Unit) {
        CloudSyncManager.oauthEvents.collect { event ->
            busy = false
            when (event) {
                is OAuthEvent.Success -> {
                    msg = "${event.provider} 授权成功"
                    dropboxStatus = CloudSyncManager.status(CloudSyncManager.Provider.DROPBOX)
                    baiduStatus = CloudSyncManager.status(CloudSyncManager.Provider.BAIDU)
                }
                is OAuthEvent.Error -> msg = "授权失败：${event.message}"
                OAuthEvent.Cancelled -> msg = "授权已取消"
            }
        }
    }

    SettingsSectionCard(title = "云同步", icon = Icons.Filled.Cloud) {
        Text(
            "将配置加密后同步到云端，在多设备间保持一致。配置用主密码加密，云端只存密文。",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(8.dp))

        // Dropbox row
        CloudProviderRow(
            name = "Dropbox",
            status = dropboxStatus,
            onConnect = {
                busy = true
                msg = "正在获取授权链接…"
                scope.launch {
                    val result = withContext(Dispatchers.IO) {
                        CloudSyncManager.authUrl(CloudSyncManager.Provider.DROPBOX)
                    }
                    busy = false
                    if (result != null) {
                        msg = "请在浏览器中完成授权"
                        val intent = Intent(Intent.ACTION_VIEW, Uri.parse(result.auth_url))
                        context.startActivity(intent)
                    } else {
                        msg = "获取授权链接失败"
                    }
                }
            },
            onUpload = {
                showUploadDialog = Pair(CloudSyncManager.Provider.DROPBOX, false)
            },
            onDownload = {
                showDownloadDialog = Pair(CloudSyncManager.Provider.DROPBOX, false)
            },
            onDisconnect = {
                scope.launch {
                    val ok = withContext(Dispatchers.IO) {
                        CloudSyncManager.disconnect(CloudSyncManager.Provider.DROPBOX)
                    }
                    if (ok) {
                        msg = "已断开 Dropbox"
                        dropboxStatus = CloudSyncManager.status(CloudSyncManager.Provider.DROPBOX)
                    }
                }
            },
        )

        HorizontalDivider(modifier = Modifier.padding(vertical = 4.dp), color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.3f))

        // Baidu row
        CloudProviderRow(
            name = "百度网盘",
            status = baiduStatus,
            onConnect = {
                busy = true
                msg = "正在获取授权链接…"
                scope.launch {
                    val result = withContext(Dispatchers.IO) {
                        CloudSyncManager.authUrl(CloudSyncManager.Provider.BAIDU)
                    }
                    busy = false
                    if (result != null) {
                        msg = "请在浏览器中完成授权"
                        val intent = Intent(Intent.ACTION_VIEW, Uri.parse(result.auth_url))
                        context.startActivity(intent)
                    } else {
                        msg = "获取授权链接失败"
                    }
                }
            },
            onUpload = {
                showUploadDialog = Pair(CloudSyncManager.Provider.BAIDU, false)
            },
            onDownload = {
                showDownloadDialog = Pair(CloudSyncManager.Provider.BAIDU, false)
            },
            onDisconnect = {
                scope.launch {
                    val ok = withContext(Dispatchers.IO) {
                        CloudSyncManager.disconnect(CloudSyncManager.Provider.BAIDU)
                    }
                    if (ok) {
                        msg = "已断开百度网盘"
                        baiduStatus = CloudSyncManager.status(CloudSyncManager.Provider.BAIDU)
                    }
                }
            },
        )

        msg?.let {
            Spacer(Modifier.height(8.dp))
            Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.primary)
        }
    }

    // === SECTION cloud_sync_ui_dialogs END ===
    // Upload password dialog (showUploadDialog: Pair<provider, isForce>)
    if (showUploadDialog != null) {
        val provider = showUploadDialog!!.first
        val isForce = showUploadDialog!!.second
        MasterPasswordDialog(
            title = if (isForce) "强行上传到云端" else "上传到云端",
            busy = busy,
            onConfirm = { pw ->
                busy = true
                val p = provider
                val f = isForce
                scope.launch {
                    val resp = withContext(Dispatchers.IO) {
                        CloudSyncManager.upload(p, pw, force = f)
                    }
                    busy = false
                    if (resp.ok) {
                        val remotePath = if (p == CloudSyncManager.Provider.BAIDU)
                            "我的应用/云盘备份/TermFast"
                        else
                            "/TermFast"
                        msg = "上传成功（${resp.size ?: 0} 字节）\n云端路径：$remotePath"
                        showUploadDialog = null
                        dropboxStatus = CloudSyncManager.status(CloudSyncManager.Provider.DROPBOX)
                        baiduStatus = CloudSyncManager.status(CloudSyncManager.Provider.BAIDU)
                    } else if (resp.conflict) {
                        // Conflict — close password dialog, show conflict confirmation
                        showUploadDialog = null
                        showConflictDialog = Pair(p, resp.reason ?: "conflict")
                    } else {
                        msg = "上传失败：${resp.message ?: resp.reason ?: "未知错误"}"
                        showUploadDialog = null
                    }
                }
            },
            onDismiss = { showUploadDialog = null },
        )
    }

    // Download password dialog (showDownloadDialog: Pair<provider, isForce>)
    if (showDownloadDialog != null) {
        val provider = showDownloadDialog!!.first
        val isForce = showDownloadDialog!!.second
        MasterPasswordDialog(
            title = if (isForce) "强行从云端下载" else "从云端下载",
            busy = busy,
            onConfirm = { pw ->
                busy = true
                val p = provider
                val f = isForce
                scope.launch {
                    val resp = withContext(Dispatchers.IO) {
                        CloudSyncManager.download(p, pw, forceDownload = f)
                    }
                    busy = false
                    when {
                        resp.ok -> {
                            msg = "下载成功：来自 ${resp.device_name ?: "未知设备"}，${resp.size ?: 0} 字节"
                            showDownloadDialog = null
                        }
                        resp.reason == "rollback_detected" -> {
                            // Close password dialog, show rollback confirmation
                            showDownloadDialog = null
                            showRollbackDialog = Pair(p, mapOf(
                                "cloud_updated_at" to resp.cloud_updated_at,
                                "last_updated_at" to resp.last_updated_at,
                                "device_name" to resp.device_name,
                            ))
                        }
                        resp.reason == "decrypt_failed" -> {
                            msg = "解密失败，主密码与云端不一致"
                        }
                        resp.reason == "no_update" -> {
                            msg = "云端无更新"
                            showDownloadDialog = null
                        }
                        resp.reason == "no_remote_data" -> {
                            msg = "云端没有同步数据"
                            showDownloadDialog = null
                        }
                        else -> {
                            msg = "下载失败：${resp.message ?: resp.reason ?: "未知错误"}"
                            showDownloadDialog = null
                        }
                    }
                }
            },
            onDismiss = { showDownloadDialog = null },
        )
    }

    // Conflict confirmation dialog — user confirms force upload
    if (showConflictDialog != null) {
        val provider = showConflictDialog!!.first
        val reason = showConflictDialog!!.second
        AlertDialog(
            onDismissRequest = { showConflictDialog = null },
            title = { Text("覆盖确认") },
            text = {
                Text(when (reason) {
                    "cloud_changed" -> "网盘文件被其他客户端改过，强行覆盖会丢失对方改动。是否继续？"
                    "cloud_exists_no_cache" -> "网盘已有数据文件，是否强行覆盖云端？"
                    else -> "是否强行覆盖云端数据？"
                })
            },
            confirmButton = {
                TextButton(onClick = {
                    showConflictDialog = null
                    // Re-open upload dialog with force=true
                    showUploadDialog = Pair(provider, true)
                }) { Text("强行覆盖") }
            },
            dismissButton = {
                TextButton(onClick = { showConflictDialog = null }) { Text("取消") }
            },
        )
    }

    // Rollback confirmation dialog — user confirms force download
    if (showRollbackDialog != null) {
        val provider = showRollbackDialog!!.first
        val info = showRollbackDialog!!.second
        AlertDialog(
            onDismissRequest = { showRollbackDialog = null },
            title = { Text("回滚警告") },
            text = {
                Text("云端文件时间戳比上次同步更旧，可能是回滚攻击。\n\n" +
                    "云端时间：${info["cloud_updated_at"] ?: "未知"}\n" +
                    "上次同步：${info["last_updated_at"] ?: "未知"}\n" +
                    "设备：${info["device_name"] ?: "未知"}\n\n" +
                    "是否强行下载？")
            },
            confirmButton = {
                TextButton(onClick = {
                    showRollbackDialog = null
                    // Re-open download dialog with force=true
                    showDownloadDialog = Pair(provider, true)
                }) { Text("强行下载") }
            },
            dismissButton = {
                TextButton(onClick = { showRollbackDialog = null }) { Text("取消") }
            },
        )
    }
}

// === SECTION cloud_sync_section_1 END ===

/** A single cloud provider row with connect/upload/download/disconnect buttons. */
@Composable
private fun CloudProviderRow(
    name: String,
    status: CloudSyncManager.SyncStatus,
    onConnect: () -> Unit,
    onUpload: () -> Unit,
    onDownload: () -> Unit,
    onDisconnect: () -> Unit,
) {
    Column(modifier = Modifier.fillMaxWidth()) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(name, style = MaterialTheme.typography.bodyLarge, fontWeight = FontWeight.Medium)
            if (status.authenticated) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Icon(
                        Icons.Filled.CloudUpload,
                        contentDescription = "上传",
                        modifier = Modifier.size(20.dp).padding(end = 2.dp),
                        tint = MaterialTheme.colorScheme.primary,
                    )
                    Spacer(Modifier.width(4.dp))
                    TextButton(onClick = onUpload, contentPadding = PaddingValues(horizontal = 8.dp)) {
                        Text("上传")
                    }
                    TextButton(onClick = onDownload, contentPadding = PaddingValues(horizontal = 8.dp)) {
                        Text("下载")
                    }
                    IconButton(onClick = onDisconnect) {
                        Icon(Icons.Filled.LinkOff, contentDescription = "断开", modifier = Modifier.size(18.dp))
                    }
                }
            } else {
                TextButton(onClick = onConnect) { Text("连接") }
            }
        }
        if (status.authenticated) {
            val syncInfo = if (status.last_synced != null) {
                "上次同步：${status.last_synced}"
            } else if (status.has_remote) {
                "云端有数据（未同步过）"
            } else {
                "云端无数据"
            }
            Text(
                syncInfo,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

/** Master password input dialog for upload/download operations. */
@Composable
private fun MasterPasswordDialog(
    title: String,
    busy: Boolean,
    onConfirm: (String) -> Unit,
    onDismiss: () -> Unit,
) {
    var password by remember { mutableStateOf("") }
    AlertDialog(
        onDismissRequest = { if (!busy) onDismiss() },
        title = { Text(title) },
        text = {
            Column {
                Text("请输入主密码以加密/解密配置", style = MaterialTheme.typography.bodySmall)
                Spacer(Modifier.height(8.dp))
                OutlinedTextField(
                    value = password,
                    onValueChange = { password = it },
                    label = { Text("主密码") },
                    singleLine = true,
                    visualTransformation = PasswordVisualTransformation(),
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
                    modifier = Modifier.fillMaxWidth(),
                    shape = RoundedCornerShape(12.dp),
                )
            }
        },
        confirmButton = {
            TextButton(
                onClick = { onConfirm(password) },
                enabled = !busy && password.isNotBlank(),
            ) { Text(if (busy) "处理中…" else "确认") }
        },
        dismissButton = {
            TextButton(onClick = onDismiss, enabled = !busy) { Text("取消") }
        },
    )
}
