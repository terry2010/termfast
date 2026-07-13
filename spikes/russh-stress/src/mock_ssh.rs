//! Mock SSH server for spike testing — FP-0.1
//!
//! Uses russh server mode to create a minimal SSH server that supports
//! password auth, exec, and direct-tcpip channels.
//! This is used for S1-S8 stress tests when no real VPS is available.

use anyhow::Result;
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

    /// Start the mock SSH server (runs forever, spawn in a task)
    pub async fn start(&self) -> Result<()> {
        // Generate a host key for the server
        let key = russh::keys::PrivateKey::random(
            &mut rand::rng(),
            russh::keys::Algorithm::Ed25519,
        )?;
        let keys = vec![key];

        let config = russh::server::Config {
            auth_rejection_time: std::time::Duration::from_secs(0),
            keys,
            ..Default::default()
        };
        let config = Arc::new(config);

        let listener = TcpListener::bind(&self.addr).await?;
        println!("mock SSH server listening on {}", self.addr);

        loop {
            let (socket, _) = listener.accept().await?;
            let config = config.clone();
            let sh = MockServerHandler {
                username: self.username.clone(),
                password: self.password.clone(),
            };
            tokio::spawn(async move {
                if let Err(e) = russh::server::run_stream(config, socket, sh).await {
                    tracing::debug!("mock SSH connection ended: {}", e);
                }
            });
        }
    }
}

// === SECTION 1 END ===

/// Mock SSH server handler
struct MockServerHandler {
    username: String,
    password: String,
}

impl russh::server::Handler for MockServerHandler {
    type Error = anyhow::Error;

    async fn auth_password(
        &mut self,
        username: &str,
        password: &str,
    ) -> Result<Auth, Self::Error> {
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
        // Echo data back
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
            "echo $SSH_CLIENT" => "1.2.3.4 12345 22\n",
            "pgrep nginx" => "12345\n",
            "apt update" => "Reading package lists... Done\n",
            _ => "mock output\n",
        };
        session.data(channel, output.as_bytes().to_vec())?;
        session.close(channel)?;
        Ok(())
    }
}

// === SECTION 2 END ===
