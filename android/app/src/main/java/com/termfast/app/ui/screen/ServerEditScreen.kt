package com.termfast.app.ui.screen

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Save
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.navigation.NavController
import com.termfast.app.data.RustRepository
import com.termfast.app.data.ServerConfig
import com.termfast.app.data.SshConfig
import com.termfast.app.data.ProxyConfig
import com.termfast.app.data.IpCheckConfig
import java.util.UUID

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ServerEditScreen(navController: NavController, serverId: String?) {
    val repo = remember { RustRepository }
    var name by remember { mutableStateOf("") }
    var host by remember { mutableStateOf("") }
    var port by remember { mutableStateOf("22") }
    var user by remember { mutableStateOf("root") }
    var authMethod by remember { mutableStateOf("password") }
    var password by remember { mutableStateOf("") }
    var keyContent by remember { mutableStateOf("") }
    var keyPassphrase by remember { mutableStateOf("") }
    var publicKey by remember { mutableStateOf("") }
    var skipHostkeyVerify by remember { mutableStateOf(false) }
    var socks5Port by remember { mutableStateOf("1080") }
    var httpPort by remember { mutableStateOf("0") }
    var mixedPort by remember { mutableStateOf("0") }
    var proxyEnabled by remember { mutableStateOf(false) }
    var loading by remember { mutableStateOf(false) }
    var existingIpCheck by remember { mutableStateOf(IpCheckConfig()) }
    var existingTriggers by remember { mutableStateOf(emptyList<com.termfast.app.data.TriggerInstance>()) }

    LaunchedEffect(serverId) {
        if (serverId != null) {
            loading = true
            val servers = repo.listServers()
            val s = servers.find { it.id == serverId }
            if (s != null) {
                name = s.name
                host = s.ssh.host
                port = s.ssh.port.toString()
                user = s.ssh.user
                authMethod = s.ssh.auth_method
                skipHostkeyVerify = s.ssh.skip_hostkey_verify
                proxyEnabled = s.proxy.enabled
                socks5Port = s.proxy.socks5_port.toString()
                httpPort = s.proxy.http_port.toString()
                mixedPort = s.proxy.mixed_port.toString()
                existingIpCheck = s.ip_check
                existingTriggers = s.triggers
                if (authMethod == "password") {
                    password = repo.loadCredential(s.id, "password") ?: ""
                } else {
                    keyContent = repo.loadCredential(s.id, "key") ?: ""
                    keyPassphrase = repo.loadCredential(s.id, "key_passphrase") ?: ""
                }
            }
            loading = false
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(if (serverId == null) "添加服务器" else "编辑服务器", fontWeight = FontWeight.SemiBold) },
                navigationIcon = {
                    IconButton(onClick = { navController.popBackStack() }) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "返回")
                    }
                }
            )
        },
        floatingActionButton = {
            FloatingActionButton(
                onClick = {
                    val id = serverId ?: UUID.randomUUID().toString()
                    val config = ServerConfig(
                        id = id,
                        name = name,
                        ssh = SshConfig(
                            host = host,
                            port = port.toIntOrNull() ?: 22,
                            user = user,
                            auth_method = authMethod,
                            skip_hostkey_verify = skipHostkeyVerify,
                        ),
                        proxy = ProxyConfig(
                            enabled = proxyEnabled,
                            socks5_port = socks5Port.toIntOrNull() ?: 1080,
                            http_port = httpPort.toIntOrNull() ?: 0,
                            mixed_port = mixedPort.toIntOrNull() ?: 0,
                        ),
                        ip_check = existingIpCheck,
                        triggers = existingTriggers,
                    )
                    if (serverId != null) {
                        // Edit existing server: update config in-place without
                        // removing the runtime instance, so an active VPN/proxy
                        // connection is not disrupted. Credentials are updated
                        // separately via saveCredential.
                        repo.saveServer(config)
                    } else {
                        // New server: add to config and create runtime instance
                        repo.addServer(config)
                    }
                    if (authMethod == "password" && password.isNotEmpty()) {
                        repo.saveCredential(id, "password", password)
                    } else {
                        if (keyContent.isNotEmpty()) repo.saveCredential(id, "key", keyContent)
                        if (keyPassphrase.isNotEmpty()) repo.saveCredential(id, "key_passphrase", keyPassphrase)
                    }
                    navController.popBackStack()
                },
                containerColor = MaterialTheme.colorScheme.primary,
                contentColor = MaterialTheme.colorScheme.onPrimary,
            ) {
                Icon(Icons.Filled.Save, contentDescription = "保存")
            }
        }
    ) { padding ->
        if (loading) {
            Box(Modifier.fillMaxSize().padding(padding), contentAlignment = Alignment.Center) {
                CircularProgressIndicator()
            }
        } else {
            Column(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(padding)
                    .verticalScroll(rememberScrollState())
                    .padding(horizontal = 16.dp, vertical = 12.dp),
                verticalArrangement = Arrangement.spacedBy(16.dp),
            ) {
                // Basic info section
                EditSectionCard(title = "基本信息") {
                    OutlinedTextField(
                        value = name, onValueChange = { name = it },
                        label = { Text("名称") },
                        modifier = Modifier.fillMaxWidth(),
                        shape = RoundedCornerShape(12.dp),
                        singleLine = true,
                    )
                    OutlinedTextField(
                        value = host, onValueChange = { host = it },
                        label = { Text("主机地址") },
                        modifier = Modifier.fillMaxWidth(),
                        shape = RoundedCornerShape(12.dp),
                        singleLine = true,
                    )
                    OutlinedTextField(
                        value = port, onValueChange = { port = it },
                        label = { Text("端口") },
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                        modifier = Modifier.fillMaxWidth(),
                        shape = RoundedCornerShape(12.dp),
                        singleLine = true,
                    )
                    OutlinedTextField(
                        value = user, onValueChange = { user = it },
                        label = { Text("用户名") },
                        modifier = Modifier.fillMaxWidth(),
                        shape = RoundedCornerShape(12.dp),
                        singleLine = true,
                    )
                }

                // Auth section
                EditSectionCard(title = "认证方式") {
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        FilterChip(
                            selected = authMethod == "password",
                            onClick = { authMethod = "password" },
                            label = { Text("密码") },
                        )
                        FilterChip(
                            selected = authMethod == "key",
                            onClick = { authMethod = "key" },
                            label = { Text("密钥") },
                        )
                    }
                    if (authMethod == "password") {
                        OutlinedTextField(
                            value = password, onValueChange = { password = it },
                            label = { Text("密码") },
                            visualTransformation = PasswordVisualTransformation(),
                            modifier = Modifier.fillMaxWidth(),
                            shape = RoundedCornerShape(12.dp),
                            singleLine = true,
                        )
                    } else {
                        OutlinedTextField(
                            value = keyContent, onValueChange = { keyContent = it },
                            label = { Text("私钥内容 (OpenSSH)") },
                            modifier = Modifier.fillMaxWidth().height(120.dp),
                            shape = RoundedCornerShape(12.dp),
                        )
                        OutlinedTextField(
                            value = keyPassphrase, onValueChange = { keyPassphrase = it },
                            label = { Text("私钥密码") },
                            visualTransformation = PasswordVisualTransformation(),
                            modifier = Modifier.fillMaxWidth(),
                            shape = RoundedCornerShape(12.dp),
                            singleLine = true,
                        )
                        if (publicKey.isNotEmpty()) {
                            OutlinedTextField(
                                value = publicKey, onValueChange = {},
                                label = { Text("公钥 (复制到 authorized_keys)") },
                                readOnly = true,
                                modifier = Modifier.fillMaxWidth().height(80.dp),
                                shape = RoundedCornerShape(12.dp),
                            )
                        }
                        OutlinedButton(
                            onClick = {
                                val id = serverId ?: UUID.randomUUID().toString()
                                val pair = repo.generateKeypair(id)
                                keyContent = pair.private_key
                                keyPassphrase = pair.passphrase
                                publicKey = pair.public_key
                            },
                            modifier = Modifier.fillMaxWidth(),
                            shape = RoundedCornerShape(12.dp),
                        ) {
                            Text("生成 Ed25519 密钥")
                        }
                    }
                    Row(
                        modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.SpaceBetween,
                    ) {
                        Text("跳过主机密钥验证", style = MaterialTheme.typography.bodyLarge)
                        Switch(checked = skipHostkeyVerify, onCheckedChange = { skipHostkeyVerify = it })
                    }
                }

                // Proxy section
                EditSectionCard(title = "代理设置") {
                    Row(
                        modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.SpaceBetween,
                    ) {
                        Text("启用 SOCKS5 代理", style = MaterialTheme.typography.bodyLarge)
                        Switch(checked = proxyEnabled, onCheckedChange = { proxyEnabled = it })
                    }
                    OutlinedTextField(
                        value = socks5Port, onValueChange = { socks5Port = it },
                        label = { Text("SOCKS5 端口") },
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                        modifier = Modifier.fillMaxWidth(),
                        shape = RoundedCornerShape(12.dp),
                        singleLine = true,
                    )
                    OutlinedTextField(
                        value = httpPort, onValueChange = { httpPort = it },
                        label = { Text("HTTP 端口 (0=禁用)") },
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                        modifier = Modifier.fillMaxWidth(),
                        shape = RoundedCornerShape(12.dp),
                        singleLine = true,
                    )
                    OutlinedTextField(
                        value = mixedPort, onValueChange = { mixedPort = it },
                        label = { Text("Mixed 端口 (0=禁用)") },
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                        modifier = Modifier.fillMaxWidth(),
                        shape = RoundedCornerShape(12.dp),
                        singleLine = true,
                    )
                }
            }
        }
    }
}

@Composable
private fun EditSectionCard(
    title: String,
    content: @Composable ColumnScope.() -> Unit,
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
            Text(title, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
            content()
        }
    }
}