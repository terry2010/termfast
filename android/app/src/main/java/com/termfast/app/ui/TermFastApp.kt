package com.termfast.app.ui

import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.clickable
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Home
import androidx.compose.material.icons.filled.Article
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Tab
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
    data object Terminals : Screen("terminals", "终端", Icons.Filled.Tab)
    data object Settings : Screen("settings", "设置", Icons.Filled.Settings)
}

// Logs is no longer in the bottom nav, but the route is still used from
//   SettingsScreen.
private const val LOGS_ROUTE = "logs"

private val screens = listOf(Screen.Servers, Screen.Terminals, Screen.Settings)

@Composable
fun TermFastApp() {
    val navController = rememberNavController()
    val backStack by navController.currentBackStackEntryAsState()
    val current = backStack?.destination
    val focusManager = LocalFocusManager.current
    val keyboardController = LocalSoftwareKeyboardController.current

    // Hide bottom nav on terminal screens (immersive mode).
    //   Note: "terminals" and "terminals_by_server" routes should KEEP the
    //   bottom bar, so we match "terminal/" specifically (the actual terminal
    //   screen routes are "terminal/{serverId}" and "terminal/{serverId}/{sessionId}").
    val currentRoute = current?.route
    val showBottomBar = currentRoute?.startsWith("terminal/") != true &&
        currentRoute?.startsWith("remote_terminal") != true

    // Determine which bottom tab should be highlighted. Routes like
    // "terminals/{focusSessionId}" and "terminals_by_server/{serverId}" should
    // highlight the "终端" tab, same as the base "terminals" route.
    val selectedRoute = when {
        currentRoute == null -> null
        currentRoute.startsWith("terminals") -> Screen.Terminals.route
        currentRoute.startsWith("servers") || currentRoute.startsWith("server_") -> Screen.Servers.route
        currentRoute.startsWith("settings") -> Screen.Settings.route
        else -> currentRoute
    }

    Scaffold(
        bottomBar = {
            if (showBottomBar) {
                NavigationBar {
                    screens.forEach { s ->
                        NavigationBarItem(
                            icon = { Icon(s.icon, contentDescription = s.label) },
                            label = { Text(s.label) },
                            selected = selectedRoute == s.route,
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
        }
    ) { inner ->
        // On terminal screens, don't apply Scaffold inner padding (immersive mode)
        val navModifier = if (showBottomBar) {
            Modifier
                .padding(inner)
                .clickable(
                    interactionSource = androidx.compose.foundation.interaction.MutableInteractionSource(),
                    indication = null,
                ) {
                    focusManager.clearFocus()
                    keyboardController?.hide()
                }
        } else {
            Modifier
        }
        NavHost(
            navController = navController,
            startDestination = Screen.Servers.route,
            modifier = navModifier,
        ) {
            composable(Screen.Servers.route) { ServerListScreen(navController) }
            composable(Screen.Terminals.route) { com.termfast.app.ui.screen.TerminalsScreen(navController) }
            composable(LOGS_ROUTE) { LogScreen() }
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
            composable("pairing") {
                com.termfast.app.ui.screen.PairingScreen(navController)
            }
            composable("qr_scanner") {
                com.termfast.app.ui.screen.QrScannerScreen(
                    onScanned = { content ->
                        navController.previousBackStackEntry?.savedStateHandle?.set("qr_result", content)
                        navController.popBackStack()
                    },
                    onBack = { navController.popBackStack() },
                )
            }
            composable("terminal/{serverId}") { backStack ->
                val id = backStack.arguments?.getString("serverId") ?: ""
                // Reuse existing connected session if available, instead of
                // creating a new one each time the user taps the terminal button.
                val existing = com.termfast.app.ui.screen.TerminalSessionManager.getSessions(id)
                    .firstOrNull { it.connected }
                if (existing != null) {
                    com.termfast.app.ui.screen.TerminalScreen(navController, id, existing.sessionId)
                } else {
                    com.termfast.app.ui.screen.TerminalScreen(navController, id)
                }
            }
            composable("terminal/{serverId}/{sessionId}") { backStack ->
                val id = backStack.arguments?.getString("serverId") ?: ""
                val sid = backStack.arguments?.getString("sessionId") ?: ""
                com.termfast.app.ui.screen.TerminalScreen(navController, id, sid)
            }
            composable("terminals/{focusSessionId}") { backStack ->
                val sid = backStack.arguments?.getString("focusSessionId") ?: ""
                com.termfast.app.ui.screen.TerminalsScreen(navController, sid)
            }
            composable("terminals_by_server/{serverId}") { backStack ->
                val serverId = backStack.arguments?.getString("serverId") ?: ""
                com.termfast.app.ui.screen.TerminalsScreen(navController, focusServerId = serverId)
            }
            // Remote terminal rendering — opens a specific remote terminal session
            composable("remote_terminal/{pairingId}/{terminalId}/{terminalName}") { backStack ->
                val pairingId = backStack.arguments?.getString("pairingId") ?: ""
                val terminalId = backStack.arguments?.getString("terminalId")?.toIntOrNull() ?: 0
                val terminalName = backStack.arguments?.getString("terminalName") ?: "Remote"
                com.termfast.app.ui.screen.RemoteTerminalScreen(
                    navController = navController,
                    pairingId = pairingId,
                    terminalId = terminalId,
                    terminalName = terminalName,
                )
            }
        }
    }
}
