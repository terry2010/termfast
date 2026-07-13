//! VPS Guard CLI — FP-6.3
//!
//! Command-line interface that connects to the daemon via socket.

mod client;

use clap::{Parser, Subcommand};
use client::DaemonClient;
use std::path::PathBuf;
use vps_guard_daemon::{Action, Response};

#[derive(Parser)]
#[command(name = "vps-guard", version, about = "VPS Guard CLI")]
struct Cli {
    /// Specify config file path (only used with --daemon)
    #[arg(long, global = true)]
    config: Option<PathBuf>,

    /// JSON output mode (for script integration)
    #[arg(long, global = true)]
    json: bool,

    /// Verbose output
    #[arg(long, short = 'v', global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start daemon (no GUI)
    Daemon,
    /// Show all server statuses
    Status,
    /// Connect to a server
    Connect { server: String },
    /// Disconnect from a server
    Disconnect { server: String },
    /// Add a new server
    AddServer {
        /// Server display name
        name: String,
        /// SSH host
        host: String,
        /// SSH port
        port: u16,
        /// SSH user
        user: String,
        /// Auth method: password or key
        #[arg(long, default_value = "password")]
        auth: String,
        /// SSH password (only for password auth)
        #[arg(long)]
        password: Option<String>,
        /// SSH key path (only for key auth)
        #[arg(long)]
        key_path: Option<String>,
        /// SOCKS5 proxy port
        #[arg(long, default_value = "1080")]
        socks5_port: u16,
        /// HTTP proxy port
        #[arg(long, default_value = "8080")]
        http_port: u16,
    },
    /// Remove a server
    RemoveServer { server: String },
    /// Save password credential for a server
    SetPassword {
        server: String,
        password: String,
    },
    /// Toggle proxy
    Proxy {
        server: String,
        /// on/off (or true/false)
        #[arg(action = clap::ArgAction::Set, value_parser = parse_on_off)]
        on: Option<bool>,
    },
    /// Manually fire a trigger
    Trigger {
        server: String,
        trigger: String,
        /// Async mode: return immediately without waiting
        #[arg(long)]
        async_mode: bool,
    },
    /// Pause all triggers
    PauseTriggers {
        #[arg(long)]
        server: Option<String>,
    },
    /// Resume triggers
    ResumeTriggers {
        #[arg(long)]
        server: Option<String>,
    },
    /// List servers
    List,
    /// List triggers for a server
    Triggers { server: String },
    /// List templates
    Templates,
    /// View logs
    Logs {
        server: Option<String>,
        #[arg(long)]
        level: Option<String>,
        #[arg(long)]
        tail: Option<usize>,
        /// Follow log output (continuous)
        #[arg(long, short = 'f')]
        follow: bool,
    },
    /// Shutdown daemon
    Shutdown,
    /// Show daemon status
    DaemonStatus,
}

// === SECTION 1 END ===

/// Parse "on"/"off"/"true"/"false" to bool
fn parse_on_off(s: &str) -> std::result::Result<bool, String> {
    match s.to_lowercase().as_str() {
        "on" | "true" | "1" => Ok(true),
        "off" | "false" | "0" => Ok(false),
        _ => Err(format!("invalid value '{}', expected on/off or true/false", s)),
    }
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // Initialize logging
    if cli.verbose {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .init();
    }

    let result = run_command(&cli).await;

    let exit_code = match result {
        Ok(()) => ExitCode::Success,
        Err(e) => {
            // Classify error into exit code 1-5 (§2.4)
            let code = classify_error(&e);
            eprintln!("Error: {}", e);
            code
        }
    };
    std::process::exit(exit_code as i32);
}

/// Exit codes (§2.4): 0=success, 1=generic, 2=auth, 3=connect, 4=config/daemon, 5=trigger
#[repr(i32)]
enum ExitCode {
    Success = 0,
    Generic = 1,
    AuthFailed = 2,
    ConnectFailed = 3,
    ConfigOrDaemon = 4,
    TriggerFailed = 5,
}

/// Classify an error into the appropriate exit code
fn classify_error(e: &anyhow::Error) -> ExitCode {
    // Check for daemon connection errors (exit 4)
    let msg = e.to_string().to_lowercase();
    if msg.contains("daemon is not running")
        || msg.contains("failed to connect to daemon")
        || msg.contains("failed to find daemon socket")
        || msg.contains("config")
        || msg.contains("migration")
    {
        return ExitCode::ConfigOrDaemon;
    }
    // Check for IpcError with specific ErrorCode
    if let Some(ipc_err) = e.downcast_ref::<vps_guard_core::error::IpcError>() {
        return match ipc_err.code {
            vps_guard_core::error::ErrorCode::AuthFailed => ExitCode::AuthFailed,
            vps_guard_core::error::ErrorCode::SshConnectFailed
            | vps_guard_core::error::ErrorCode::SshDisconnected => ExitCode::ConnectFailed,
            vps_guard_core::error::ErrorCode::TriggerCommandFailed => ExitCode::TriggerFailed,
            vps_guard_core::error::ErrorCode::ConfigCorrupt
            | vps_guard_core::error::ErrorCode::ConfigMigrationFailed
            | vps_guard_core::error::ErrorCode::InvalidParams => ExitCode::ConfigOrDaemon,
            _ => ExitCode::Generic,
        };
    }
    // Check error message for auth/connect keywords (daemon IpcError doesn't impl Display)
    if msg.contains("auth") || msg.contains("authentication") {
        return ExitCode::AuthFailed;
    }
    if msg.contains("connect") || msg.contains("ssh") || msg.contains("connection") {
        return ExitCode::ConnectFailed;
    }
    if msg.contains("trigger") {
        return ExitCode::TriggerFailed;
    }
    ExitCode::Generic
}

async fn run_command(cli: &Cli) -> anyhow::Result<()> {
    match &cli.command {
        Commands::Daemon => {
            // Start daemon in headless mode
            start_daemon(cli.config.as_deref()).await
        }
        Commands::Shutdown => {
            let mut client = DaemonClient::connect().await?;
            let resp = client.send_simple(Action::Shutdown).await?;
            print_response(&resp, cli.json);
            Ok(())
        }
        Commands::DaemonStatus => {
            let mut client = DaemonClient::connect().await?;
            let resp = client.send_simple(Action::GetDaemonStatus).await?;
            print_response(&resp, cli.json);
            Ok(())
        }
        Commands::Status | Commands::List => {
            let mut client = DaemonClient::connect().await?;
            let resp = client.send_simple(Action::ListServers).await?;
            print_response(&resp, cli.json);
            Ok(())
        }
        Commands::Connect { server } => {
            let mut client = DaemonClient::connect().await?;
            let server_id = client.resolve_server_id(server.as_str()).await?;
            let resp = client
                .send_request(Action::ConnectServer, serde_json::json!({"server_id": server_id}))
                .await?;
            print_response(&resp, cli.json);
            Ok(())
        }
        Commands::Disconnect { server } => {
            let mut client = DaemonClient::connect().await?;
            let server_id = client.resolve_server_id(server.as_str()).await?;
            let resp = client
                .send_request(Action::DisconnectServer, serde_json::json!({"server_id": server_id}))
                .await?;
            print_response(&resp, cli.json);
            Ok(())
        }
        Commands::AddServer { name, host, port, user, auth, password, key_path, socks5_port, http_port } => {
            let mut client = DaemonClient::connect().await?;
            let server_id = format!("srv_{}", chrono::Utc::now().timestamp_millis());
            let config = serde_json::json!({
                "id": server_id,
                "name": name,
                "ssh": {
                    "host": host,
                    "port": port,
                    "user": user,
                    "auth_method": auth,
                    "key_path": key_path.clone().unwrap_or_default(),
                    "connection_mode": "single",
                    "skip_hostkey_verify": false,
                    "key_auto_generated": false,
                },
                "proxy": {
                    "socks5_port": socks5_port,
                    "http_port": http_port,
                    "enabled": false,
                    "max_channels": 64,
                    "channel_idle_timeout": 300,
                },
                "reconnect": {
                    "max_attempts": 10,
                    "initial_backoff_secs": 1,
                    "max_backoff_secs": 300,
                    "heartbeat_interval": 15,
                },
                "ip_check": {
                    "enabled": true,
                    "interval_secs": 300,
                },
                "triggers": [],
                "suppress_firewall_badge": false,
            });
            let resp = client.send_request(Action::AddServer, config).await?;
            // Save password credential if provided
            if let Some(pwd) = password {
                if !pwd.is_empty() {
                    let _ = client.send_request(Action::SaveCredential, serde_json::json!({
                        "server_id": server_id,
                        "credential_type": "password",
                        "value": pwd,
                    })).await;
                }
            }
            print_response(&resp, cli.json);
            Ok(())
        }
        Commands::RemoveServer { server } => {
            let mut client = DaemonClient::connect().await?;
            let server_id = client.resolve_server_id(server.as_str()).await?;
            let resp = client
                .send_request(Action::RemoveServer, serde_json::json!({"server_id": server_id}))
                .await?;
            print_response(&resp, cli.json);
            Ok(())
        }
        Commands::SetPassword { server, password } => {
            let mut client = DaemonClient::connect().await?;
            let server_id = client.resolve_server_id(server.as_str()).await?;
            let resp = client
                .send_request(Action::SaveCredential, serde_json::json!({
                    "server_id": server_id,
                    "credential_type": "password",
                    "value": password,
                }))
                .await?;
            print_response(&resp, cli.json);
            Ok(())
        }
        Commands::PauseTriggers { server } => {
            let mut client = DaemonClient::connect().await?;
            let action = if server.is_some() {
                Action::PauseServerTriggers
            } else {
                Action::PauseAllTriggers
            };
            let params = if let Some(s) = server {
                let sid = client.resolve_server_id(s.as_str()).await?;
                serde_json::json!({"server_id": sid})
            } else {
                serde_json::Value::Null
            };
            let resp = client.send_request(action, params).await?;
            print_response(&resp, cli.json);
            Ok(())
        }
        Commands::ResumeTriggers { server } => {
            let mut client = DaemonClient::connect().await?;
            let action = if server.is_some() {
                Action::ResumeServerTriggers
            } else {
                Action::ResumeAllTriggers
            };
            let params = if let Some(s) = server {
                let sid = client.resolve_server_id(s.as_str()).await?;
                serde_json::json!({"server_id": sid})
            } else {
                serde_json::Value::Null
            };
            let resp = client.send_request(action, params).await?;
            print_response(&resp, cli.json);
            Ok(())
        }
        Commands::Logs { server, level, tail, follow } => {
            let mut client = DaemonClient::connect().await?;
            let mut params = serde_json::json!({});
            if let Some(s) = server {
                let sid = client.resolve_server_id(s.as_str()).await?;
                params["server_id"] = serde_json::Value::String(sid);
            }
            if let Some(l) = level {
                params["level"] = serde_json::Value::String(l.clone());
            }
            if let Some(t) = tail {
                params["limit"] = serde_json::Value::Number((*t as u64).into());
            }
            let resp = client.send_request(Action::GetLogs, params).await?;
            print_response(&resp, cli.json);

            if *follow {
                client.follow_events(cli.json).await?;
            }
            Ok(())
        }
        Commands::Triggers { server } => {
            let mut client = DaemonClient::connect().await?;
            let server_id = client.resolve_server_id(server.as_str()).await?;
            // Get config and extract triggers for this server
            let resp = client.send_simple(Action::GetConfig).await?;
            if let Response::Ok { data, .. } = &resp {
                if let Some(servers) = data["servers"].as_array() {
                    if let Some(srv) = servers.iter().find(|s| s["id"].as_str() == Some(server_id.as_str())) {
                        let triggers = &srv["triggers"];
                        let status = srv["current_status"].as_str().unwrap_or("unknown");
                        if cli.json {
                            print_response(&Response::Ok { id: String::new(), data: triggers.clone() }, true);
                        } else {
                            println!("Server status: {}", status);
                            if let Some(arr) = triggers.as_array() {
                                if arr.is_empty() {
                                    println!("No triggers configured.");
                                } else {
                                    for t in arr {
                                        let name = t["name"].as_str().unwrap_or("?");
                                        let id = t["id"].as_str().unwrap_or("?");
                                        let ttype = t["trigger_type"].as_str().unwrap_or("?");
                                        let enabled = t["enabled"].as_bool().unwrap_or(false);
                                        let cmds = t["commands"].as_array().map(|a| a.len()).unwrap_or(0);
                                        println!("  {} [{}] type={} enabled={} commands={}", name, id, ttype, enabled, cmds);
                                    }
                                }
                            }
                        }
                        return Ok(());
                    }
                }
            }
            println!("Server not found.");
            Ok(())
        }
        Commands::Templates => {
            let mut client = DaemonClient::connect().await?;
            let resp = client.send_simple(Action::ListTemplates).await?;
            print_response(&resp, cli.json);
            Ok(())
        }
        Commands::Proxy { server, on } => {
            let mut client = DaemonClient::connect().await?;
            let server_id = client.resolve_server_id(server.as_str()).await?;
            let enabled = on.unwrap_or(true);
            let resp = client
                .send_request(
                    Action::ToggleProxy,
                    serde_json::json!({"server_id": server_id, "enabled": enabled}),
                )
                .await?;
            print_response(&resp, cli.json);
            Ok(())
        }
        Commands::Trigger {
            server,
            trigger,
            async_mode,
        } => {
            let mut client = DaemonClient::connect().await?;
            let server_id = client.resolve_server_id(server.as_str()).await?;
            let trigger_id = client.resolve_trigger_id(&server_id, trigger.as_str()).await?;
            let resp = client
                .send_request(
                    Action::ManualFireTrigger,
                    serde_json::json!({
                        "server_id": server_id,
                        "trigger_id": trigger_id,
                        "async": async_mode,
                    }),
                )
                .await?;
            print_response(&resp, cli.json);
            Ok(())
        }
    }
}

// === SECTION 2 END ===

/// Print a daemon response (JSON or human-readable)
fn print_response(resp: &vps_guard_daemon::Response, json: bool) {
    match resp {
        vps_guard_daemon::Response::Ok { data, .. } => {
            if json {
                println!("{}", serde_json::to_string_pretty(data).unwrap_or_default());
            } else {
                print_data_human(data);
            }
        }
        vps_guard_daemon::Response::Err { error, .. } => {
            eprintln!("Error [{}]: {}", error.code_str(), error.detail);
        }
        vps_guard_daemon::Response::Event { event, data } => {
            if json {
                println!(
                    "{{\"event\":\"{}\",\"data\":{}}}",
                    event,
                    serde_json::to_string(data).unwrap_or_default()
                );
            } else {
                println!("[{}] {}", event, serde_json::to_string_pretty(data).unwrap_or_default());
            }
        }
    }
}

/// Print data in human-readable format using comfy-table for tabular data
fn print_data_human(data: &serde_json::Value) {
    match data {
        serde_json::Value::Null => {}
        serde_json::Value::Bool(b) => println!("{}", b),
        serde_json::Value::Number(n) => println!("{}", n),
        serde_json::Value::String(s) => println!("{}", s),
        serde_json::Value::Array(arr) => {
            if arr.is_empty() {
                println!("(empty)");
                return;
            }
            // Try to render as a table if items are objects
            if let Some(table) = build_table(arr) {
                println!("{table}");
            } else {
                for item in arr {
                    println!("{}", serde_json::to_string_pretty(item).unwrap_or_default());
                }
            }
        }
        serde_json::Value::Object(_) => {
            println!("{}", serde_json::to_string_pretty(data).unwrap_or_default());
        }
    }
}

/// Build a comfy-table from an array of JSON objects
fn build_table(arr: &[serde_json::Value]) -> Option<comfy_table::Table> {
    use comfy_table::{Table, ContentArrangement, presets::UTF8_FULL};
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);

    // Extract headers from first object
    let first = arr.first()?;
    let obj = first.as_object()?;
    let headers: Vec<String> = obj.keys().cloned().collect();
    table.set_header(&headers);

    // Add rows
    for item in arr {
        if let Some(item_obj) = item.as_object() {
            let row: Vec<String> = headers
                .iter()
                .map(|h| {
                    item_obj
                        .get(h)
                        .map(value_to_string)
                        .unwrap_or_default()
                })
                .collect();
            table.add_row(row);
        }
    }
    Some(table)
}

/// Convert a JSON value to a short display string
fn value_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => "-".into(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            serde_json::to_string(v).unwrap_or_default()
        }
    }
}

/// Start the daemon in headless mode
async fn start_daemon(config_path: Option<&std::path::Path>) -> anyhow::Result<()> {
    use vps_guard_core::config::ConfigManager;
    use vps_guard_daemon::{DaemonServer, DaemonState};

    // Load config from specified path, or use default
    let mgr = if let Some(path) = config_path {
        ConfigManager::load(path)
            .map_err(|e| anyhow::anyhow!("failed to load config from {}: {}", path.display(), e))?
    } else {
        ConfigManager::new(vps_guard_core::config::Config::default())
    };
    let state = DaemonState::new(mgr);

    let server = DaemonServer::start(state).await?;

    println!("VPS Guard daemon started (headless mode)");
    println!("Socket: {}", server.socket_path().display());

    // Wait for shutdown signal
    tokio::signal::ctrl_c().await?;

    println!("\nShutting down daemon...");
    server.shutdown().await;
    println!("Daemon stopped.");

    Ok(())
}

/// Helper trait for error code string representation
trait ErrorCodeStr {
    fn code_str(&self) -> &str;
}

impl ErrorCodeStr for vps_guard_daemon::IpcError {
    fn code_str(&self) -> &str {
        // Return the actual error code name from the ErrorCode enum
        match self.code {
            vps_guard_core::error::ErrorCode::PortConflict => "PortConflict",
            vps_guard_core::error::ErrorCode::AuthFailed => "AuthFailed",
            vps_guard_core::error::ErrorCode::SshConnectFailed => "SshConnectFailed",
            vps_guard_core::error::ErrorCode::SshDisconnected => "SshDisconnected",
            vps_guard_core::error::ErrorCode::HostKeyMismatch => "HostKeyMismatch",
            vps_guard_core::error::ErrorCode::ConfigCorrupt => "ConfigCorrupt",
            vps_guard_core::error::ErrorCode::ConfigMigrationFailed => "ConfigMigrationFailed",
            vps_guard_core::error::ErrorCode::CredentialNotFound => "CredentialNotFound",
            vps_guard_core::error::ErrorCode::CredentialStoreFailed => "CredentialStoreFailed",
            vps_guard_core::error::ErrorCode::TemplateNotFound => "TemplateNotFound",
            vps_guard_core::error::ErrorCode::TriggerNotFound => "TriggerNotFound",
            vps_guard_core::error::ErrorCode::ServerNotFound => "ServerNotFound",
            vps_guard_core::error::ErrorCode::ProxyPortInUse => "ProxyPortInUse",
            vps_guard_core::error::ErrorCode::NeedsPrivilege => "NeedsPrivilege",
            vps_guard_core::error::ErrorCode::ImportFailed => "ImportFailed",
            vps_guard_core::error::ErrorCode::DecryptionFailed => "DecryptionFailed",
            vps_guard_core::error::ErrorCode::TriggerCommandFailed => "TriggerCommandFailed",
            vps_guard_core::error::ErrorCode::Internal => "Internal",
            vps_guard_core::error::ErrorCode::InvalidParams => "InvalidParams",
        }
    }
}
