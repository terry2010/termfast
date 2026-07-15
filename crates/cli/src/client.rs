//! Daemon socket client — FP-6.3
//!
//! Connects to the daemon via Unix socket and sends/receives IPC messages.

use anyhow::{bail, Result};
use termfast_daemon::frame;
use termfast_daemon::{Action, Request, Response};

/// CLI daemon client
pub struct DaemonClient {
    #[cfg(unix)]
    stream: tokio::net::UnixStream,
}

impl DaemonClient {
    /// Connect to the running daemon
    pub async fn connect() -> Result<Self> {
        let socket_path = termfast_daemon::find_daemon_socket()
            .map_err(|e| anyhow::anyhow!("failed to find daemon socket: {}", e))?;

        let socket_path = match socket_path {
            Some(path) => path,
            None => {
                bail!("daemon is not running. Start it with `termfast --daemon` or launch the GUI")
            }
        };

        #[cfg(unix)]
        {
            let stream = tokio::net::UnixStream::connect(&socket_path)
                .await
                .map_err(|e| {
                    anyhow::anyhow!("failed to connect to daemon socket {}: {}", socket_path, e)
                })?;
            Ok(Self { stream })
        }

        #[cfg(not(unix))]
        bail!("Windows named pipe client not yet implemented")
    }

    /// Send a request and wait for the response
    pub async fn send_request(
        &mut self,
        action: Action,
        params: serde_json::Value,
    ) -> Result<Response> {
        // Tag CLI requests so daemon can broadcast focus events to GUI
        let mut params = params;
        if let Some(obj) = params.as_object_mut() {
            obj.insert("_cli".to_string(), serde_json::Value::Bool(true));
        } else {
            params = serde_json::json!({ "_cli": true, "_data": params });
        }
        let request = Request::new(action, params);
        let request_data = serde_json::to_vec(&request)?;

        #[cfg(unix)]
        {
            let (mut read_half, mut write_half) = self.stream.split();

            // Send request
            frame::write_frame(&mut write_half, &request_data).await?;

            // Read response
            let response_data = frame::read_frame(&mut read_half).await?;
            let response: Response = serde_json::from_slice(&response_data)?;
            Ok(response)
        }

        #[cfg(not(unix))]
        bail!("not supported on this platform")
    }

    /// Send a simple request (no params)
    pub async fn send_simple(&mut self, action: Action) -> Result<Response> {
        self.send_request(action, serde_json::Value::Null).await
    }

    /// Resolve a server name or ID to a server_id (§2.4)
    /// If the input matches a server ID directly, returns it.
    /// Otherwise, looks up the server by name (case-insensitive).
    pub async fn resolve_server_id(&mut self, name_or_id: &str) -> Result<String> {
        // First, try as-is (might be a server_id)
        let resp = self.send_simple(Action::GetConfig).await?;
        if let Response::Ok { data, .. } = resp {
            if let Some(servers) = data["servers"].as_array() {
                // Exact ID match
                for s in servers {
                    if s["id"].as_str() == Some(name_or_id) {
                        return Ok(name_or_id.to_string());
                    }
                }
                // Name match (case-insensitive)
                let lower = name_or_id.to_lowercase();
                for s in servers {
                    if let Some(name) = s["name"].as_str() {
                        if name.to_lowercase() == lower {
                            return Ok(s["id"].as_str().unwrap_or("").to_string());
                        }
                    }
                }
            }
        }
        // If no match found, return the input as-is (let the daemon handle the error)
        Ok(name_or_id.to_string())
    }

    /// Resolve a trigger name or ID to a trigger_id for a given server
    pub async fn resolve_trigger_id(
        &mut self,
        server_id: &str,
        name_or_id: &str,
    ) -> Result<String> {
        let resp = self.send_simple(Action::GetConfig).await?;
        if let Response::Ok { data, .. } = resp {
            if let Some(servers) = data["servers"].as_array() {
                if let Some(server) = servers.iter().find(|s| s["id"].as_str() == Some(server_id)) {
                    if let Some(triggers) = server["triggers"].as_array() {
                        // Exact ID match
                        for t in triggers {
                            if t["id"].as_str() == Some(name_or_id) {
                                return Ok(name_or_id.to_string());
                            }
                        }
                        // Name match (case-insensitive)
                        let lower = name_or_id.to_lowercase();
                        for t in triggers {
                            if let Some(name) = t["name"].as_str() {
                                if name.to_lowercase() == lower {
                                    return Ok(t["id"].as_str().unwrap_or("").to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(name_or_id.to_string())
    }

    /// Continuously read events from the socket (for --follow mode)
    pub async fn follow_events(&mut self, json: bool) -> Result<()> {
        #[cfg(unix)]
        {
            let (mut read_half, _write_half) = self.stream.split();
            loop {
                // Read next frame (blocks until data available)
                let data = match frame::read_frame(&mut read_half).await {
                    Ok(data) => data,
                    Err(e)
                        if e.to_string().contains("EOF")
                            || e.to_string().contains("unexpected end") =>
                    {
                        break
                    }
                    Err(e) => return Err(e),
                };

                if let Ok(Response::Event {
                    ref event,
                    ref data,
                }) = serde_json::from_slice::<Response>(&data)
                {
                    if json {
                        println!(
                            "{{\"event\":\"{}\",\"data\":{}}}",
                            event,
                            serde_json::to_string(data).unwrap_or_default()
                        );
                    } else {
                        println!(
                            "[{}] {}",
                            event,
                            serde_json::to_string_pretty(data).unwrap_or_default()
                        );
                    }
                }
            }
        }
        Ok(())
    }
}

// === SECTION 1 END ===
