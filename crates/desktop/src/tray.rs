//! System tray menu structure — FP-6.4
//!
//! Data model for tray menu. Actual Tauri integration in desktop lib.

use serde::{Deserialize, Serialize};

/// Tray icon color based on global health
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrayIconColor {
    /// All servers connected and healthy
    Green,
    /// Some servers reconnecting
    Yellow,
    /// Some servers have auth failure or errors
    Red,
    /// All servers disconnected
    Gray,
}

/// Tray menu item
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TrayMenuItem {
    /// Plain text label
    Label { text: String },
    /// Separator line
    Separator,
    /// Server submenu
    Server {
        id: String,
        name: String,
        status: String,
        proxy_enabled: bool,
    },
    /// Action button
    Action { id: String, label: String },
    /// Submenu
    Submenu { label: String, items: Vec<TrayMenuItem> },
}

/// Tray menu structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrayMenu {
    pub items: Vec<TrayMenuItem>,
    pub icon_color: TrayIconColor,
    pub badge: u32,
}

/// Calculate tray icon color from server statuses
pub fn calculate_icon_color(
    statuses: &[(String, String)], // (server_id, status)
) -> TrayIconColor {
    if statuses.is_empty() {
        return TrayIconColor::Gray;
    }

    let has_error = statuses.iter().any(|(_, s)| {
        s == "auth_failed" || s == "error"
    });
    let has_reconnecting = statuses.iter().any(|(_, s)| {
        s == "reconnecting" || s == "connecting"
    });
    let has_connected = statuses.iter().any(|(_, s)| s == "connected");
    let all_disconnected = statuses.iter().all(|(_, s)| s == "disconnected");

    if has_error {
        TrayIconColor::Red
    } else if has_reconnecting {
        TrayIconColor::Yellow
    } else if all_disconnected {
        TrayIconColor::Gray
    } else if has_connected {
        TrayIconColor::Green
    } else {
        TrayIconColor::Gray
    }
}

/// Calculate badge number (count of non-connected servers)
pub fn calculate_badge(statuses: &[(String, String)]) -> u32 {
    statuses
        .iter()
        .filter(|(_, s)| s != "connected" && s != "disconnected")
        .count() as u32
}

/// Build the tray menu from server list
pub fn build_tray_menu(
    servers: &[(String, String, bool)], // (id, name, proxy_enabled)
    statuses: &[(String, String)],      // (id, status)
) -> TrayMenu {
    let icon_color = calculate_icon_color(statuses);
    let badge = calculate_badge(statuses);

    let mut items = Vec::new();

    // Server submenus
    let mut server_items: Vec<TrayMenuItem> = Vec::new();
    for (id, name, proxy_enabled) in servers {
        let status = statuses
            .iter()
            .find(|(sid, _)| sid == id)
            .map(|(_, s)| s.as_str())
            .unwrap_or("disconnected");
        server_items.push(TrayMenuItem::Server {
            id: id.clone(),
            name: name.clone(),
            status: status.to_string(),
            proxy_enabled: *proxy_enabled,
        });
    }

    if !server_items.is_empty() {
        items.push(TrayMenuItem::Submenu {
            label: "Servers".into(),
            items: server_items,
        });
        items.push(TrayMenuItem::Separator);
    }

    items.push(TrayMenuItem::Action {
        id: "connect_all".into(),
        label: "Connect All".into(),
    });
    items.push(TrayMenuItem::Action {
        id: "disconnect_all".into(),
        label: "Disconnect All".into(),
    });
    items.push(TrayMenuItem::Separator);
    items.push(TrayMenuItem::Action {
        id: "show_window".into(),
        label: "Show Main Window".into(),
    });
    items.push(TrayMenuItem::Action {
        id: "quit".into(),
        label: "Quit".into(),
    });

    TrayMenu {
        items,
        icon_color,
        badge,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_icon_color_all_connected() {
        let statuses = vec![
            ("srv_1".into(), "connected".into()),
            ("srv_2".into(), "connected".into()),
        ];
        assert_eq!(calculate_icon_color(&statuses), TrayIconColor::Green);
    }

    #[test]
    fn test_icon_color_has_reconnecting() {
        let statuses = vec![
            ("srv_1".into(), "connected".into()),
            ("srv_2".into(), "reconnecting".into()),
        ];
        assert_eq!(calculate_icon_color(&statuses), TrayIconColor::Yellow);
    }

    #[test]
    fn test_icon_color_has_error() {
        let statuses = vec![
            ("srv_1".into(), "connected".into()),
            ("srv_2".into(), "auth_failed".into()),
        ];
        assert_eq!(calculate_icon_color(&statuses), TrayIconColor::Red);
    }

    #[test]
    fn test_icon_color_empty() {
        let statuses = vec![];
        assert_eq!(calculate_icon_color(&statuses), TrayIconColor::Gray);
    }

    #[test]
    fn test_badge_calculation() {
        let statuses = vec![
            ("srv_1".into(), "connected".into()),
            ("srv_2".into(), "reconnecting".into()),
            ("srv_3".into(), "disconnected".into()),
        ];
        assert_eq!(calculate_badge(&statuses), 1); // only reconnecting counts
    }

    #[test]
    fn test_build_tray_menu() {
        let servers = vec![
            ("srv_1".into(), "Tokyo".into(), true),
            ("srv_2".into(), "US West".into(), false),
        ];
        let statuses = vec![
            ("srv_1".into(), "connected".into()),
            ("srv_2".into(), "disconnected".into()),
        ];
        let menu = build_tray_menu(&servers, &statuses);
        assert_eq!(menu.icon_color, TrayIconColor::Green);
        assert_eq!(menu.badge, 0);
        assert!(menu.items.len() >= 4); // submenu + separator + actions
    }
}
