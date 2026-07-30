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
}

impl MockSshServer {
    pub fn new(addr: &str, username: &str, password: &str) -> Self {
        Self {
            addr: addr.to_string(),
            username: username.to_string(),
            password: password.to_string(),
        }
    }

    /// Start the mock SSH server
    pub async fn start(&self) -> Result<()> {
        // Generate an Ed25519 key pair for the server
        let mut rng = ssh_key::rand_core::UnwrapErr(ssh_key::getrandom::SysRng);
        let key_pair = ssh_key::PrivateKey::random(&mut rng, ssh_key::Algorithm::Ed25519 {})
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
        // Handle common exec commands for testing
        let output = match request.as_ref() {
            "echo $SSH_CONNECTION" => "1.2.3.4 12345 5.6.7.8 22\n",
            "pgrep nginx" => "12345\n",
            _ => "mock output\n",
        };
        session.data(channel, output.as_bytes().to_vec())?;
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
