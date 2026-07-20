package com.termfast.app.ui

import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.clickable
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Home
import androidx.compose.material.icons.filled.Article
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.Icon
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.navigation.NavDestination.Companion.hierarchy
import androidx.navigation.NavGraph.Companion.findStartDestination
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import com.termfast.app.ui.screen.LogScreen
import com.termfast.app.ui.screen.ServerListScreen
import com.termfast.app.ui.screen.SettingsScreen

sealed class Screen(val route: String, val label: String, val icon: androidx.compose.ui.graphics.vector.ImageVector) {
    data object Servers : Screen("servers", "服务器", Icons.Filled.Home)
    data object Logs : Screen("logs", "日志", Icons.Filled.Article)
    data object Settings : Screen("settings", "设置", Icons.Filled.Settings)
}

private val screens = listOf(Screen.Servers, Screen.Logs, Screen.Settings)

@Composable
fun TermFastApp() {
    val navController = rememberNavController()
    val backStack by navController.currentBackStackEntryAsState()
    val current = backStack?.destination
    val focusManager = LocalFocusManager.current
    val keyboardController = LocalSoftwareKeyboardController.current

    Scaffold(
        bottomBar = {
            NavigationBar {
                screens.forEach { s ->
                    NavigationBarItem(
                        icon = { Icon(s.icon, contentDescription = s.label) },
                        label = { Text(s.label) },
                        selected = current?.hierarchy?.any { it.route == s.route } == true,
                        onClick = {
                            navController.navigate(s.route) {
                                popUpTo(navController.graph.findStartDestination().id) {
                                    saveState = true
                                }
                                launchSingleTop = true
                                restoreState = true
                            }
                        }
                    )
                }
            }
        }
    ) { inner ->
        NavHost(
            navController = navController,
            startDestination = Screen.Servers.route,
            modifier = Modifier
                .padding(inner)
                .clickable(
                    interactionSource = androidx.compose.foundation.interaction.MutableInteractionSource(),
                    indication = null,
                ) {
                    focusManager.clearFocus()
                    keyboardController?.hide()
                }
        ) {
            composable(Screen.Servers.route) { ServerListScreen(navController) }
            composable(Screen.Logs.route) { LogScreen() }
            composable(Screen.Settings.route) { SettingsScreen(navController) }
            composable("server_detail/{serverId}") { backStack ->
                val id = backStack.arguments?.getString("serverId") ?: ""
                com.termfast.app.ui.screen.ServerDetailScreen(navController, id)
            }
            composable("server_add") {
                com.termfast.app.ui.screen.ServerEditScreen(navController, null)
            }
            composable("server_edit/{serverId}") { backStack ->
                val id = backStack.arguments?.getString("serverId") ?: ""
                com.termfast.app.ui.screen.ServerEditScreen(navController, id)
            }
            composable("trigger_edit/{serverId}/{triggerId}") { backStack ->
                val serverId = backStack.arguments?.getString("serverId") ?: ""
                val triggerId = backStack.arguments?.getString("triggerId")
                com.termfast.app.ui.screen.TriggerEditScreen(navController, serverId, triggerId)
            }
            composable("per_app_proxy") {
                com.termfast.app.ui.screen.PerAppProxyScreen(navController)
            }
            composable("terminal/{serverId}") { backStack ->
                val id = backStack.arguments?.getString("serverId") ?: ""
                com.termfast.app.ui.screen.TerminalScreen(navController, id)
            }
            composable("terminal/{serverId}/{sessionId}") { backStack ->
                val id = backStack.arguments?.getString("serverId") ?: ""
                val sid = backStack.arguments?.getString("sessionId") ?: ""
                com.termfast.app.ui.screen.TerminalScreen(navController, id, sid)
            }
        }
    }
}
