//! Trigger engine — FP-4.2
//!
//! Event queue + command execution scheduling (per-server Mutex).
//! Supports: OnConnect, OnReconnect, OnIpChange, OnProcessDead, OnPortClosed, ManualFire.
//! Features: per-server serialization, cooldown, continue_on_error, timeout.

use crate::config::{TriggerInstance, TriggerTemplate, TriggerType};
use crate::error::Result;
use crate::ssh::client::SshClientHandle;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// Trigger event — fired when a specific condition occurs
#[derive(Debug, Clone)]
pub struct TriggerEvent {
    pub server_id: String,
    pub server_name: String,
    pub trigger_type: TriggerType,
    pub new_ip: Option<String>,
    pub old_ip: Option<String>,
}

/// Trigger execution result — returned after executing a trigger
#[derive(Debug, Clone)]
pub struct TriggerExecutionResult {
    pub trigger_id: String,
    pub trigger_name: String,
    pub success: bool,
    pub executed_commands: usize,
    pub total_commands: usize,
    pub results: Vec<CommandResult>,
}

/// Single command execution result
#[derive(Debug, Clone)]
pub struct CommandResult {
    pub command: String,
    pub exit_code: u32,
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
}

/// Cooldown tracking key: (server_id, trigger_id)
type CooldownKey = (String, String);

/// Pending event — stored while triggers are paused (§10.3)
#[derive(Debug, Clone)]
pub struct PendingEvent {
    pub server_id: String,
    pub server_name: String,
    pub trigger_type: TriggerType,
    pub new_ip: Option<String>,
    pub old_ip: Option<String>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Trigger engine — manages trigger execution per server
pub struct TriggerEngine {
    /// Per-server execution locks (prevent concurrent trigger execution for same server)
    locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    /// Paused state per server
    paused: Mutex<HashMap<String, bool>>,
    /// Global pause
    global_paused: Mutex<bool>,
    /// Cooldown tracking: (server_id, trigger_id) -> last fired Instant
    cooldowns: Mutex<HashMap<CooldownKey, Instant>>,
    /// Number of currently running trigger executions
    running_count: Mutex<u32>,
    /// Pending events accumulated while paused (§10.3)
    pending_events: Mutex<Vec<PendingEvent>>,
    /// User-defined custom variables (injected into every trigger execution)
    custom_variables: Mutex<Vec<crate::config::CustomVariable>>,
}

impl TriggerEngine {
    pub fn new() -> Self {
        Self {
            locks: Mutex::new(HashMap::new()),
            paused: Mutex::new(HashMap::new()),
            global_paused: Mutex::new(false),
            cooldowns: Mutex::new(HashMap::new()),
            running_count: Mutex::new(0),
            pending_events: Mutex::new(Vec::new()),
            custom_variables: Mutex::new(Vec::new()),
        }
    }

    /// Update custom variables (called when config changes)
    pub async fn set_custom_variables(&self, vars: Vec<crate::config::CustomVariable>) {
        *self.custom_variables.lock().await = vars;
    }

    /// Check if any trigger is currently running
    pub async fn has_running(&self) -> bool {
        *self.running_count.lock().await > 0
    }

    /// Check if triggers are paused for a server
    pub async fn is_paused(&self, server_id: &str) -> bool {
        let global = *self.global_paused.lock().await;
        if global {
            return true;
        }
        let paused = self.paused.lock().await;
        *paused.get(server_id).unwrap_or(&false)
    }

    /// Pause all triggers
    pub async fn pause_all(&self) {
        *self.global_paused.lock().await = true;
    }

    /// Resume all triggers and return pending events (§10.3)
    /// The caller should display these pending events to the user.
    pub async fn resume_all(&self) -> Vec<PendingEvent> {
        *self.global_paused.lock().await = false;
        // Drain and return all pending events
        let mut pending = self.pending_events.lock().await;
        std::mem::take(&mut *pending)
    }

    /// Get pending events without draining (for UI display while still paused)
    pub async fn get_pending_events(&self) -> Vec<PendingEvent> {
        self.pending_events.lock().await.clone()
    }

    /// Add a pending event (called when triggers are paused and an event occurs)
    pub async fn add_pending_event(&self, event: PendingEvent) {
        self.pending_events.lock().await.push(event);
    }

    /// Pause triggers for a specific server
    pub async fn pause_server(&self, server_id: &str) {
        self.paused.lock().await.insert(server_id.to_string(), true);
    }

    /// Resume triggers for a specific server
    pub async fn resume_server(&self, server_id: &str) {
        self.paused.lock().await.insert(server_id.to_string(), false);
    }

    /// Check if a trigger is in cooldown
    async fn is_in_cooldown(&self, server_id: &str, trigger_id: &str, cooldown_secs: u64) -> bool {
        if cooldown_secs == 0 {
            return false;
        }
        let cooldowns = self.cooldowns.lock().await;
        if let Some(last_fired) = cooldowns.get(&(server_id.to_string(), trigger_id.to_string())) {
            let elapsed = last_fired.elapsed();
            return elapsed < Duration::from_secs(cooldown_secs);
        }
        false
    }

    /// Record that a trigger was fired (for cooldown tracking)
    async fn record_fire(&self, server_id: &str, trigger_id: &str) {
        self.cooldowns.lock().await.insert(
            (server_id.to_string(), trigger_id.to_string()),
            Instant::now(),
        );
    }

    /// Fire triggers for an event.
    /// Looks up matching triggers from the server's trigger instances + templates,
    /// filters by event type, checks cooldown, then executes matching triggers.
    pub async fn fire_event(
        &self,
        ssh: &SshClientHandle,
        triggers: &[TriggerInstance],
        templates: &[TriggerTemplate],
        event: &TriggerEvent,
    ) -> Result<Vec<TriggerExecutionResult>> {
        if self.is_paused(&event.server_id).await {
            tracing::info!(
                "triggers paused for server {}, skipping {:?}",
                event.server_id,
                event.trigger_type
            );
            return Ok(Vec::new());
        }

        // Filter triggers that match this event type
        let matching_triggers: Vec<&TriggerInstance> = triggers
            .iter()
            .filter(|t| t.enabled)
            .filter(|t| {
                // Use instance's trigger_type if set; otherwise look up template
                let ttype = if t.trigger_type != TriggerType::ManualFire || t.template_id.is_empty() {
                    t.trigger_type.clone()
                } else {
                    // Look up template to get trigger type
                    let template = templates.iter().find(|tmpl| tmpl.id == t.template_id);
                    match template {
                        Some(tmpl) => tmpl.trigger_type.clone(),
                        None => TriggerType::ManualFire,
                    }
                };
                ttype == event.trigger_type
            })
            .collect();

        if matching_triggers.is_empty() {
            tracing::debug!(
                "no matching triggers for server {} event {:?}",
                event.server_id,
                event.trigger_type
            );
            return Ok(Vec::new());
        }

        // Get or create per-server lock
        let lock = {
            let mut locks = self.locks.lock().await;
            locks
                .entry(event.server_id.clone())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };

        let _guard = lock.lock().await;

        let mut results = Vec::new();
        for trigger in matching_triggers {
            // Check cooldown (skip for OnConnect/OnReconnect — user-initiated events)
            let skip_cooldown = matches!(event.trigger_type, TriggerType::OnConnect | TriggerType::OnReconnect);
            if !skip_cooldown && self.is_in_cooldown(&event.server_id, &trigger.id, trigger.cooldown_secs).await {
                tracing::info!(
                    "trigger {} for server {} is in cooldown, skipping",
                    trigger.id,
                    event.server_id
                );
                continue;
            }

            // Record fire for cooldown
            self.record_fire(&event.server_id, &trigger.id).await;

            let result = self.execute_trigger(ssh, trigger, event).await;
            let should_break = !result.success && !trigger.continue_on_error;
            results.push(result);

            if should_break {
                break;
            }
        }

        Ok(results)
    }

    /// Manually fire a specific trigger by ID.
    /// Finds the trigger in the server's trigger list and executes it.
    pub async fn manual_fire(
        &self,
        server_id: &str,
        server_name: &str,
        trigger_id: &str,
        ssh: &SshClientHandle,
        triggers: &[TriggerInstance],
    ) -> Result<TriggerExecutionResult> {
        if self.is_paused(server_id).await {
            return Err(crate::error::Error::Other(format!(
                "triggers paused for server {}",
                server_id
            )));
        }

        // Find the trigger
        let trigger = triggers
            .iter()
            .find(|t| t.id == trigger_id)
            .ok_or_else(|| {
                crate::error::Error::Other(format!(
                    "trigger {} not found for server {}",
                    trigger_id,
                    server_id
                ))
            })?;

        if !trigger.enabled {
            return Err(crate::error::Error::Other(format!(
                "trigger {} is disabled",
                trigger_id
            )));
        }

        // Get per-server lock
        let lock = {
            let mut locks = self.locks.lock().await;
            locks
                .entry(server_id.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };

        let _guard = lock.lock().await;

        // Record fire for cooldown
        self.record_fire(server_id, trigger_id).await;

        let event = TriggerEvent {
            server_id: server_id.to_string(),
            server_name: server_name.to_string(),
            trigger_type: TriggerType::ManualFire,
            new_ip: None,
            old_ip: None,
        };

        Ok(self.execute_trigger(ssh, trigger, &event).await)
    }

    /// Execute a single trigger — runs all its commands sequentially
    async fn execute_trigger(
        &self,
        ssh: &SshClientHandle,
        trigger: &TriggerInstance,
        event: &TriggerEvent,
    ) -> TriggerExecutionResult {
        // Track running count for graceful drain
        *self.running_count.lock().await += 1;
        let _guard = RunningGuard::new(self);

        let mut vars = std::collections::HashMap::new();
        vars.insert("ServerName".to_string(), event.server_name.clone());
        if let Some(ref ip) = event.new_ip {
            vars.insert("NewIP".to_string(), ip.clone());
            vars.insert("IPFamily".to_string(), crate::ssh::exec::ip_family(ip).to_string());
        }
        if let Some(ref ip) = event.old_ip {
            vars.insert("OldIP".to_string(), ip.clone());
        }
        // Add user parameters (from trigger instance, e.g. ProtectedPort)
        for (k, v) in &trigger.parameters {
            vars.insert(k.clone(), v.clone());
        }
        // Add user-defined custom variables (from global config)
        // These are injected last so they don't override system variables,
        // but CAN be overridden by trigger-specific parameters above.
        let custom_vars = self.custom_variables.lock().await;
        for cv in custom_vars.iter() {
            vars.entry(cv.name.clone()).or_insert_with(|| cv.value.clone());
        }
        drop(custom_vars);

        let total = trigger.commands.len();
        let mut command_results = Vec::new();
        let mut success = true;

        for cmd_template in &trigger.commands {
            let rendered = crate::trigger::template::render_template(cmd_template, &vars);
            tracing::info!("executing trigger command: {}", rendered);

            match ssh.exec(&rendered, trigger.timeout_secs).await {
                Ok(exec_result) => {
                    let cmd_success = exec_result.is_success();
                    command_results.push(CommandResult {
                        command: rendered,
                        exit_code: exec_result.exit_code,
                        stdout: exec_result.stdout,
                        stderr: exec_result.stderr,
                        success: cmd_success,
                    });
                    if !cmd_success {
                        success = false;
                        if !trigger.continue_on_error {
                            break;
                        }
                    }
                }
                Err(e) => {
                    command_results.push(CommandResult {
                        command: rendered,
                        exit_code: 255,
                        stdout: String::new(),
                        stderr: e.to_string(),
                        success: false,
                    });
                    success = false;
                    if !trigger.continue_on_error {
                        break;
                    }
                }
            }
        }

        TriggerExecutionResult {
            trigger_id: trigger.id.clone(),
            trigger_name: trigger.name.clone(),
            success,
            executed_commands: command_results.len(),
            total_commands: total,
            results: command_results,
        }
    }
}

impl Default for TriggerEngine {
    fn default() -> Self {
        Self::new()
    }
}

// === SECTION 1 END ===

/// RAII guard to decrement running_count when trigger execution completes
struct RunningGuard<'a>(&'a TriggerEngine);

impl<'a> RunningGuard<'a> {
    fn new(engine: &'a TriggerEngine) -> Self {
        Self(engine)
    }
}

impl<'a> Drop for RunningGuard<'a> {
    fn drop(&mut self) {
        // Use try_lock to avoid deadlock; if locked, we skip (best-effort)
        if let Ok(mut count) = self.0.running_count.try_lock() {
            if *count > 0 {
                *count -= 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_pause_resume_all() {
        let engine = TriggerEngine::new();
        assert!(!engine.is_paused("srv_1").await);
        engine.pause_all().await;
        assert!(engine.is_paused("srv_1").await);
        engine.resume_all().await;
        assert!(!engine.is_paused("srv_1").await);
    }

    #[tokio::test]
    async fn test_pause_resume_server() {
        let engine = TriggerEngine::new();
        assert!(!engine.is_paused("srv_1").await);
        engine.pause_server("srv_1").await;
        assert!(engine.is_paused("srv_1").await);
        engine.resume_server("srv_1").await;
        assert!(!engine.is_paused("srv_1").await);
    }

    #[tokio::test]
    async fn test_cooldown_tracking() {
        let engine = TriggerEngine::new();
        // Not in cooldown initially
        assert!(!engine.is_in_cooldown("srv_1", "trig_1", 60).await);
        // Record fire
        engine.record_fire("srv_1", "trig_1").await;
        // Now in cooldown (60s)
        assert!(engine.is_in_cooldown("srv_1", "trig_1", 60).await);
        // Zero cooldown = never in cooldown
        assert!(!engine.is_in_cooldown("srv_1", "trig_1", 0).await);
    }

    #[tokio::test]
    async fn test_cooldown_different_triggers() {
        let engine = TriggerEngine::new();
        engine.record_fire("srv_1", "trig_1").await;
        // trig_2 is not in cooldown even though trig_1 is
        assert!(!engine.is_in_cooldown("srv_1", "trig_2", 60).await);
        // trig_1 on different server is not in cooldown
        assert!(!engine.is_in_cooldown("srv_2", "trig_1", 60).await);
    }

    #[test]
    fn test_trigger_event_construction() {
        let event = TriggerEvent {
            server_id: "srv_1".to_string(),
            server_name: "My VPS".to_string(),
            trigger_type: TriggerType::OnConnect,
            new_ip: Some("1.2.3.4".to_string()),
            old_ip: None,
        };
        assert_eq!(event.server_id, "srv_1");
        assert_eq!(event.trigger_type, TriggerType::OnConnect);
        assert_eq!(event.new_ip.as_deref(), Some("1.2.3.4"));
        assert!(event.old_ip.is_none());
    }

    #[test]
    fn test_trigger_execution_result() {
        let result = TriggerExecutionResult {
            trigger_id: "trig_1".to_string(),
            trigger_name: "firewalld".to_string(),
            success: true,
            executed_commands: 3,
            total_commands: 3,
            results: vec![CommandResult {
                command: "echo hello".to_string(),
                exit_code: 0,
                stdout: "hello\n".to_string(),
                stderr: String::new(),
                success: true,
            }],
        };
        assert!(result.success);
        assert_eq!(result.executed_commands, 3);
        assert_eq!(result.results.len(), 1);
        assert!(result.results[0].success);
    }
}

// === SECTION 2 END ===
