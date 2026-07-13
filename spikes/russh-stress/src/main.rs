//! russh stress test spike — FP-0.1
//!
//! Go/No-Go milestone: verify russh can handle real-world proxy workloads.
//! S1-S9 test scenarios from design doc §20.0.
//!
//! Usage:
//!   VPS_GUARD_TEST_HOST=1.2.3.4 VPS_GUARD_TEST_USER=root VPS_GUARD_TEST_PASS=xxx \
//!   cargo run --bin russh-stress -- --scenario S1

use anyhow::Result;
use clap::Parser;
use std::env;

mod mock_ssh;
mod stress_test;

/// Spike test scenarios
#[derive(Parser, Debug)]
struct Args {
    /// Scenario to run (S1-S9, or "all")
    #[arg(short, long, default_value = "all")]
    scenario: String,

    /// Mock SSH server port (for scenarios that don't need real VPS)
    #[arg(long, default_value = "2222")]
    mock_port: u16,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    dotenvy::dotenv().ok();

    let args = Args::parse();

    println!("russh stress test spike — FP-0.1");
    println!("scenario: {}", args.scenario);

    // Start mock SSH server if no real VPS is configured
    let using_real_vps = env::var("VPS_GUARD_TEST_HOST").is_ok();
    if !using_real_vps {
        println!("starting mock SSH server on port {}", args.mock_port);
        let mock_addr = format!("127.0.0.1:{}", args.mock_port);
        let mock_server = mock_ssh::MockSshServer::new(&mock_addr, "testuser", "testpass");
        tokio::spawn(async move {
            if let Err(e) = mock_server.start().await {
                eprintln!("mock SSH server error: {}", e);
            }
        });
        // Wait for server to start
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        println!("mock SSH server started");
    } else {
        println!("using real VPS from VPS_GUARD_TEST_HOST");
    }
    println!();

    match args.scenario.as_str() {
        "S1" => stress_test::s1_concurrent_channels().await?,
        "S2" => stress_test::s2_large_file().await?,
        "S3" => stress_test::s3_browser_tabs().await?,
        "S4" => stress_test::s4_long_running().await?,
        "S5" => stress_test::s5_mixed_channels().await?,
        "S6" => stress_test::s6_slow_remote().await?,
        "S7" => stress_test::s7_long_command().await?,
        "S8" => stress_test::s8_reconnect_100().await?,
        "S9" => stress_test::s9_ring_only_compile().await?,
        "all" => {
            for s in ["S1", "S2", "S3", "S4", "S5", "S6", "S7", "S8", "S9"] {
                println!("\n=== Running {} ===", s);
                match s {
                    "S1" => stress_test::s1_concurrent_channels().await,
                    "S2" => stress_test::s2_large_file().await,
                    "S3" => stress_test::s3_browser_tabs().await,
                    "S4" => stress_test::s4_long_running().await,
                    "S5" => stress_test::s5_mixed_channels().await,
                    "S6" => stress_test::s6_slow_remote().await,
                    "S7" => stress_test::s7_long_command().await,
                    "S8" => stress_test::s8_reconnect_100().await,
                    "S9" => stress_test::s9_ring_only_compile().await,
                    _ => unreachable!(),
                }?;
            }
        }
        _ => anyhow::bail!("unknown scenario: {}", args.scenario),
    }

    println!("\n=== Spike complete ===");
    Ok(())
}

/// Get VPS test credentials from environment
pub fn get_test_credentials() -> Option<TestCreds> {
    let host = env::var("VPS_GUARD_TEST_HOST").ok()?;
    let port = env::var("VPS_GUARD_TEST_PORT").ok().unwrap_or_else(|| "22".into());
    let user = env::var("VPS_GUARD_TEST_USER").ok().unwrap_or_else(|| "root".into());
    let pass = env::var("VPS_GUARD_TEST_PASS").ok()?;
    Some(TestCreds { host, port: port.parse().unwrap_or(22), user, pass })
}

pub struct TestCreds {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub pass: String,
}
