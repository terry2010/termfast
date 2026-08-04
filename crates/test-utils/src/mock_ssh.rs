//! Mock SSH server for testing — FP-2.0
//!
//! Uses russh server mode to create a minimal SSH server that supports
//! password auth, key auth, exec, and direct-tcpip channels.

use anyhow::Result;
use russh::keys::ssh_key;
use russh::server::{Auth, ChannelOpenHandle, Msg, Session};
use russh::{Channel, ChannelId};
use std::sync::Arc;
use tokio::net::TcpListener;

/// Mock SSH server configuration
pub struct MockSshServer {
    addr: String,
    username: String,
    password: String,
    /// Optional tmux simulation state (shared via Arc for handler clones)
    tmux_sim: Arc<std::sync::Mutex<TmuxSim>>,
}

/// Tmux simulation state for mock SSH server
#[derive(Default)]
pub struct TmuxSim {
    /// Whether tmux is "installed"
    pub installed: bool,
    /// Pre-configured session list output (raw lines from `tmux list-sessions -F`)
    pub sessions_output: String,
    /// Set of session names that "exist" (for has-session checks)
    pub existing_names: std::collections::HashSet<String>,
    /// If true, `tmux has-session` always returns exit 0 (simulate all names collide)
    pub has_session_always_true: bool,
    /// Log of all exec commands received (for test assertions)
    pub exec_log: std::sync::Mutex<Vec<String>>,
}

impl MockSshServer {
    pub fn new(addr: &str, username: &str, password: &str) -> Self {
        Self {
            addr: addr.to_string(),
            username: username.to_string(),
            password: password.to_string(),
            tmux_sim: Arc::new(std::sync::Mutex::new(TmuxSim::default())),
        }
    }

    /// Configure tmux simulation: set installed=true and provide session list output
    pub fn with_tmux(self, installed: bool, sessions_output: &str) -> Self {
        {
            let mut sim = self.tmux_sim.lock().unwrap();
            sim.installed = installed;
            sim.sessions_output = sessions_output.to_string();
            // Parse session names from output (first field before |)
            for line in sessions_output.lines() {
                if let Some(name) = line.split('|').next() {
                    sim.existing_names.insert(name.to_string());
                }
            }
        }
        self
    }

    /// Configure tmux simulation where `has-session` always returns true
    /// (simulates all generated names colliding — for testing retry-then-Err path)
    pub fn with_tmux_always_exists(self) -> Self {
        {
            let mut sim = self.tmux_sim.lock().unwrap();
            sim.installed = true;
            sim.has_session_always_true = true;
        }
        self
    }

    /// Get a clone of the exec log for test assertions
    pub fn get_exec_log(&self) -> Vec<String> {
        let sim = self.tmux_sim.lock().unwrap();
        let log = sim.exec_log.lock().unwrap();
        log.clone()
    }

    /// Get a clone of the tmux_sim Arc for sharing with test assertions
    pub fn tmux_sim_handle(&self) -> Arc<std::sync::Mutex<TmuxSim>> {
        self.tmux_sim.clone()
    }

    /// Start the mock SSH server
    pub async fn start(&self) -> Result<()> {
        // Generate an Ed25519 key pair for the server
        let key_pair = ssh_key::PrivateKey::random(&mut rand::rng(), ssh_key::Algorithm::Ed25519)
            .map_err(|e| anyhow::anyhow!("key generation failed: {}", e))?;

        let config = russh::server::Config {
            auth_rejection_time: std::time::Duration::from_secs(0),
            keys: vec![key_pair],
            ..Default::default()
        };
        let config = Arc::new(config);

        let listener = TcpListener::bind(&self.addr).await?;
        tracing::info!("mock SSH server listening on {}", self.addr);

        loop {
            let (socket, _) = listener.accept().await?;
            let config = config.clone();
            let sh = MockServerHandler {
                username: self.username.clone(),
                password: self.password.clone(),
                tmux_sim: self.tmux_sim.clone(),
            };
            tokio::spawn(async move {
                let _ = russh::server::run_stream(config, socket, sh).await;
            });
        }
    }
}

/// Mock SSH server handler
struct MockServerHandler {
    username: String,
    password: String,
    tmux_sim: Arc<std::sync::Mutex<TmuxSim>>,
}

impl russh::server::Handler for MockServerHandler {
    type Error = anyhow::Error;

    async fn auth_password(&mut self, username: &str, password: &str) -> Result<Auth, Self::Error> {
        if username == self.username && password == self.password {
            Ok(Auth::Accept)
        } else {
            Ok(Auth::reject())
        }
    }

    async fn channel_open_session(
        &mut self,
        _channel: Channel<Msg>,
        reply: ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        Ok(())
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        _channel: Channel<Msg>,
        _host_to_connect: &str,
        _port_to_connect: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        Ok(())
    }

    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        // Echo data back for testing
        session.data(channel, data.to_vec())?;
        Ok(())
    }

    async fn exec_request(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        let request = String::from_utf8_lossy(data);
        let sim = self.tmux_sim.lock().unwrap();

        // Log the exec command for test assertions
        {
            let mut log = sim.exec_log.lock().unwrap();
            log.push(request.to_string());
        }

        // Handle common exec commands for testing
        let (mut output, exit_code): (Vec<u8>, u32) = if request.starts_with("command -v tmux") {
            if sim.installed {
                ("/usr/bin/tmux\n".as_bytes().to_vec(), 0)
            } else {
                (Vec::new(), 1) // not found
            }
        } else if request.starts_with("tmux list-sessions") {
            if sim.sessions_output.is_empty() {
                (Vec::new(), 1) // no sessions
            } else {
                (sim.sessions_output.as_bytes().to_vec(), 0)
            }
        } else if request.starts_with("tmux has-session") {
            if sim.has_session_always_true {
                // always exists — simulate all collisions
                // Append EXIT:0 for the echo fallback
                (b"EXIT:0\n".to_vec(), 0)
            } else {
                // Extract session name from -t 'xxx' or -t xxx
                let name = extract_tmux_target(&request);
                if let Some(n) = name {
                    if sim.existing_names.contains(&n) {
                        (b"EXIT:0\n".to_vec(), 0)
                    } else {
                        (b"EXIT:1\n".to_vec(), 1)
                    }
                } else {
                    (b"EXIT:1\n".to_vec(), 1)
                }
            }
        } else if request.starts_with("tmux new -s") || request.starts_with("tmux set-option") || request.starts_with("tmux kill-session") {
            // Simulate success for create/configure/kill commands
            (Vec::new(), 0)
        } else {
            match request.as_ref() {
                "echo $SSH_CONNECTION" => ("1.2.3.4 12345 5.6.7.8 22\n".as_bytes().to_vec(), 0),
                "pgrep nginx" => ("12345\n".as_bytes().to_vec(), 0),
                _ => ("mock output\n".as_bytes().to_vec(), 0),
            }
        };

        // If the command contains "; echo EXIT:$?" but output doesn't have it,
        // append the exit marker (for commands that don't explicitly add it)
        if request.contains("EXIT:$?") && !String::from_utf8_lossy(&output).contains("EXIT:") {
            output.extend_from_slice(format!("EXIT:{}\n", exit_code).as_bytes());
        }

        session.data(channel, output)?;
        // Always send exit status (even for 0) so exec.rs can detect it
        let _ = session.exit_status_request(channel, exit_code);
        session.close(channel)?;
        Ok(())
    }

    async fn tcpip_forward(
        &mut self,
        address: &str,
        port: &mut u32,
        _session: &mut Session,
    ) -> Result<bool, Self::Error> {
        // Accept the tcpip_forward request.
        // If port is 0, assign a fixed test port for deterministic tests.
        if *port == 0 {
            *port = 12345;
        }
        tracing::info!("mock server: tcpip_forward {} accepted on port {}", address, port);
        // Note: A full end-to-end test would require the mock server to
        // open forwarded-tcpip channels back to the client when connections
        // arrive on the forwarded port. However, russh 0.62's Session is
        // not Send and cannot be used from a spawned task, making it
        // impossible to simulate incoming connections in the mock server.
        // The RemoteForwarder's data forwarding logic (run_receive_loop +
        // copy_bidirectional_remote) is structurally identical to
        // LocalForwarder's (run_accept_loop + copy_bidirectional_local),
        // which is fully tested in test_local_forward_data_transfer.
        Ok(true)
    }

    async fn cancel_tcpip_forward(
        &mut self,
        address: &str,
        port: u32,
        _session: &mut Session,
    ) -> Result<bool, Self::Error> {
        tracing::info!("mock server: cancel_tcpip_forward {}:{}", address, port);
        Ok(true)
    }
}

/// Extract the session name from `tmux has-session -t 'name'` or `-t name`
fn extract_tmux_target(cmd: &str) -> Option<String> {
    let t_idx = cmd.find("-t ")?;
    let after_t = &cmd[t_idx + 3..];
    let after_t = after_t.trim_start();
    if after_t.starts_with('\'') {
        // Quoted: 'name'
        let end = after_t[1..].find('\'')?;
        Some(after_t[1..1 + end].to_string())
    } else {
        // Unquoted: take until whitespace
        let end = after_t.find(|c: char| c.is_whitespace()).unwrap_or(after_t.len());
        Some(after_t[..end].to_string())
    }
}
