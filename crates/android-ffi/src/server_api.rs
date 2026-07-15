//! Server lifecycle API stub.
//!
//! Full implementation will manage `ServerConfig`/`ServerInstance` via
//! `termfast-core::server`.

use termfast_core::config::ServerConfig;

pub fn add_server_stub(config: ServerConfig) -> String {
    config.id.to_string()
}

pub fn list_servers_stub() -> Vec<ServerConfig> {
    vec![]
}
