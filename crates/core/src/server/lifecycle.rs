//! Connection state machine — FP-5.3
//!
//! Manages connect/disconnect/reconnect state transitions.

use crate::server::instance::ServerStatus;

/// Connection state machine
pub struct ConnectionStateMachine {
    state: ServerStatus,
}

impl ConnectionStateMachine {
    pub fn new() -> Self {
        Self {
            state: ServerStatus::Disconnected,
        }
    }

    pub fn state(&self) -> &ServerStatus {
        &self.state
    }

    /// Transition to a new state. Returns false if transition is invalid.
    pub fn transition(&mut self, new_state: ServerStatus) -> bool {
        let valid = match (&self.state, &new_state) {
            // From Disconnected
            (ServerStatus::Disconnected, ServerStatus::Connecting) => true,
            // From Connecting
            (ServerStatus::Connecting, ServerStatus::Connected) => true,
            (ServerStatus::Connecting, ServerStatus::AuthFailed) => true,
            (ServerStatus::Connecting, ServerStatus::Reconnecting) => true,
            (ServerStatus::Connecting, ServerStatus::Error) => true,
            (ServerStatus::Connecting, ServerStatus::Disconnected) => true,
            // From Connected
            (ServerStatus::Connected, ServerStatus::Reconnecting) => true,
            (ServerStatus::Connected, ServerStatus::Disconnected) => true,
            (ServerStatus::Connected, ServerStatus::Error) => true,
            // From Reconnecting
            (ServerStatus::Reconnecting, ServerStatus::Connected) => true,
            (ServerStatus::Reconnecting, ServerStatus::Disconnected) => true,
            (ServerStatus::Reconnecting, ServerStatus::Error) => true,
            (ServerStatus::Reconnecting, ServerStatus::AuthFailed) => true,
            // From AuthFailed
            (ServerStatus::AuthFailed, ServerStatus::Disconnected) => true,
            (ServerStatus::AuthFailed, ServerStatus::Connecting) => true,
            // From Error
            (ServerStatus::Error, ServerStatus::Disconnected) => true,
            (ServerStatus::Error, ServerStatus::Connecting) => true,
            // Same state
            (a, b) if a == b => true,
            _ => false,
        };

        if valid {
            self.state = new_state;
        }
        valid
    }
}

impl Default for ConnectionStateMachine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state() {
        let sm = ConnectionStateMachine::new();
        assert_eq!(*sm.state(), ServerStatus::Disconnected);
    }

    #[test]
    fn test_valid_transitions() {
        let mut sm = ConnectionStateMachine::new();
        assert!(sm.transition(ServerStatus::Connecting));
        assert!(sm.transition(ServerStatus::Connected));
        assert!(sm.transition(ServerStatus::Reconnecting));
        assert!(sm.transition(ServerStatus::Connected));
        assert!(sm.transition(ServerStatus::Disconnected));
    }

    #[test]
    fn test_invalid_transition() {
        let mut sm = ConnectionStateMachine::new();
        // Disconnected -> Connected is invalid (must go through Connecting)
        assert!(!sm.transition(ServerStatus::Connected));
        assert_eq!(*sm.state(), ServerStatus::Disconnected);
    }

    #[test]
    fn test_auth_failed_transition() {
        let mut sm = ConnectionStateMachine::new();
        assert!(sm.transition(ServerStatus::Connecting));
        assert!(sm.transition(ServerStatus::AuthFailed));
        assert!(sm.transition(ServerStatus::Disconnected));
    }
}
