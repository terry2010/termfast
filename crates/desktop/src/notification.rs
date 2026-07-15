//! Notification system — FP-6.8
//!
//! Notification preferences and dispatch logic.

use serde::{Deserialize, Serialize};

/// Notification category
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationCategory {
    Connection,
    Proxy,
    Trigger,
    Config,
}

/// Notification level (three tiers)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationLevel {
    /// System notification + tray color change
    SystemAndTray,
    /// Tray color change only
    TrayOnly,
    /// Log only (no visible notification)
    LogOnly,
}

/// Notification preferences per category
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationPrefs {
    pub connection: NotificationLevel,
    pub proxy: NotificationLevel,
    pub trigger: NotificationLevel,
    pub config: NotificationLevel,
}

impl Default for NotificationPrefs {
    fn default() -> Self {
        // Default: anomalies on, normal behavior off (§9.5)
        Self {
            // Connection: disconnect/auth failure → system; connect success → log only
            connection: NotificationLevel::SystemAndTray,
            // Proxy: port conflict → system; normal toggle → log only
            proxy: NotificationLevel::LogOnly,
            // Trigger: execution failure → system; success → log only
            trigger: NotificationLevel::SystemAndTray,
            // Config: errors → system; normal changes → log only
            config: NotificationLevel::LogOnly,
        }
    }
}

impl NotificationPrefs {
    /// Get the notification level for a category
    pub fn level_for(&self, category: &NotificationCategory) -> &NotificationLevel {
        match category {
            NotificationCategory::Connection => &self.connection,
            NotificationCategory::Proxy => &self.proxy,
            NotificationCategory::Trigger => &self.trigger,
            NotificationCategory::Config => &self.config,
        }
    }

    /// Check if a notification should be sent as a system notification
    pub fn should_notify_system(&self, category: &NotificationCategory) -> bool {
        matches!(self.level_for(category), NotificationLevel::SystemAndTray)
    }

    /// Check if the tray should change color
    pub fn should_change_tray(&self, category: &NotificationCategory) -> bool {
        matches!(
            self.level_for(category),
            NotificationLevel::SystemAndTray | NotificationLevel::TrayOnly
        )
    }
}

/// A notification event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationEvent {
    pub category: NotificationCategory,
    pub title: String,
    pub body: String,
    pub server_id: Option<String>,
    pub severity: NotificationSeverity,
}

/// Notification severity
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationSeverity {
    Info,
    Warning,
    Error,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_prefs() {
        let prefs = NotificationPrefs::default();
        assert_eq!(prefs.connection, NotificationLevel::SystemAndTray);
        assert_eq!(prefs.proxy, NotificationLevel::LogOnly);
        assert_eq!(prefs.trigger, NotificationLevel::SystemAndTray);
        assert_eq!(prefs.config, NotificationLevel::LogOnly);
    }

    #[test]
    fn test_should_notify_system() {
        let prefs = NotificationPrefs::default();
        assert!(prefs.should_notify_system(&NotificationCategory::Connection));
        assert!(!prefs.should_notify_system(&NotificationCategory::Proxy));
        assert!(prefs.should_notify_system(&NotificationCategory::Trigger));
    }

    #[test]
    fn test_should_change_tray() {
        let prefs = NotificationPrefs::default();
        assert!(prefs.should_change_tray(&NotificationCategory::Connection));
        assert!(!prefs.should_change_tray(&NotificationCategory::Proxy));
        assert!(prefs.should_change_tray(&NotificationCategory::Trigger));
    }

    #[test]
    fn test_level_for_each_category() {
        let prefs = NotificationPrefs::default();
        assert_eq!(
            prefs.level_for(&NotificationCategory::Connection),
            &NotificationLevel::SystemAndTray
        );
        assert_eq!(
            prefs.level_for(&NotificationCategory::Proxy),
            &NotificationLevel::LogOnly
        );
    }
}
