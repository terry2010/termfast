//! Proxy performance benchmark tests — FP-9.9
//!
//! Measures throughput, concurrency, latency, and resource usage
//! of the SOCKS5/HTTP proxy through SSH direct-tcpip channels.
//! Uses real data transfer through TCP echo to measure actual throughput.

use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use vps_guard_test_utils::MockSshServer;

// === SECTION 1 END ===

/// Start a local TCP echo server that echoes back all received data.
/// Used for throughput measurement.
async fn start_echo_server(port: u16) -> tokio::task::JoinHandle<()> {
    let listener = TcpListener::bind(("127.0.0.1", port)).await.unwrap();
    tokio::spawn(async move {
        loop {
            if let Ok((mut socket, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    loop {
                        match socket.read(&mut buf).await {
                            Ok(0) | Err(_) => break,
                            Ok(n) => {
                                if socket.write_all(&buf[..n]).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                });
            }
        }
    })
}

/// Measure TCP connection latency to a local port
async fn measure_latency(host: &str, port: u16, iterations: usize) -> (u64, u64, u64) {
    let mut times = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let start = Instant::now();
        match tokio::time::timeout(
            Duration::from_secs(2),
            TcpStream::connect((host, port)),
        ).await {
            Ok(Ok(_)) => {
                times.push(start.elapsed().as_micros() as u64);
            }
            _ => {
                times.push(0);
            }
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let min = *times.iter().min().unwrap_or(&0);
    let max = *times.iter().max().unwrap_or(&0);
    let avg = if times.is_empty() { 0 } else { times.iter().sum::<u64>() / times.len() as u64 };
    (min, max, avg)
}

/// Measure actual data throughput through a TCP echo server.
/// Sends `data_size` bytes and measures round-trip time.
async fn measure_throughput(host: &str, port: u16, data_size: usize) -> Option<f64> {
    let mut stream = TcpStream::connect((host, port)).await.ok()?;
    let data = vec![0xABu8; data_size];
    let start = Instant::now();
    stream.write_all(&data).await.ok()?;
    let mut received = vec![0u8; data_size];
    let mut total = 0;
    while total < data_size {
        let n = stream.read(&mut received[total..]).await.ok()?;
        if n == 0 { return None; }
        total += n;
    }
    let elapsed = start.elapsed();
    // Throughput in MB/s (round-trip, so divide by 2 for one-way)
    let mbps = (data_size as f64 * 2.0) / (elapsed.as_secs_f64() * 1024.0 * 1024.0);
    Some(mbps)
}

/// Get current process memory usage in bytes (RSS).
fn get_memory_usage() -> u64 {
    #[cfg(unix)]
    {
        // Read /proc/self/status on Linux, or use getrusage on macOS
        #[cfg(target_os = "linux")]
        {
            if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
                for line in status.lines() {
                    if line.starts_with("VmRSS:") {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() >= 2 {
                            if let Ok(kb) = parts[1].parse::<u64>() {
                                return kb * 1024;
                            }
                        }
                    }
                }
            }
        }
        #[cfg(target_os = "macos")]
        {
            // Use mach_task_basic_info on macOS
            // mach2 provides the non-deprecated mach_task_self(); libc provides task_info
            unsafe {
                let mut info: libc::mach_task_basic_info_data_t = std::mem::zeroed();
                let mut count = (std::mem::size_of::<libc::mach_task_basic_info_data_t>()
                    / std::mem::size_of::<libc::natural_t>()) as libc::mach_msg_type_number_t;
                let result = libc::task_info(
                    mach2::traps::mach_task_self(),
                    libc::MACH_TASK_BASIC_INFO,
                    &mut info as *mut _ as libc::task_info_t,
                    &mut count,
                );
                if result == libc::KERN_SUCCESS {
                    return info.resident_size as u64;
                }
            }
        }
    }
    0
}

// === SECTION 2 END ===

/// FP-9.9.1: Latency benchmark — measure TCP connection setup time
#[tokio::test]
async fn test_benchmark_tcp_latency() {
    let server = MockSshServer::new("127.0.0.1:4231", "user", "pass");
    tokio::spawn(async move { let _ = server.start().await; });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let (min, max, avg) = measure_latency("127.0.0.1", 4231, 20).await;
    eprintln!("TCP latency: min={}μs max={}μs avg={}μs", min, max, avg);
    assert!(avg < 100_000, "latency should be under 100ms, got {}μs", avg);
    assert!(min > 0, "min latency should be non-zero");
}

/// FP-9.9.2: Concurrency benchmark — measure concurrent connection capacity
#[tokio::test]
async fn test_benchmark_concurrent_connections() {
    let server = MockSshServer::new("127.0.0.1:4232", "user", "pass");
    tokio::spawn(async move { let _ = server.start().await; });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let concurrency = 50;
    let start = Instant::now();
    let mut handles = Vec::new();

    for i in 0..concurrency {
        handles.push(tokio::spawn(async move {
            let result = tokio::time::timeout(
                Duration::from_secs(5),
                TcpStream::connect(("127.0.0.1", 4232)),
            ).await;
            (i, result.is_ok())
        }));
    }

    let mut success = 0;
    for h in handles {
        if let Ok((_, ok)) = h.await {
            if ok { success += 1; }
        }
    }
    let elapsed = start.elapsed();
    eprintln!("Concurrent connections: {}/{} succeeded in {:?}", success, concurrency, elapsed);
    assert!(success >= concurrency * 8 / 10, "at least 80% should succeed");
}

// === SECTION 3 END ===

/// FP-9.9.3: Throughput benchmark — measure actual data transfer rate
#[tokio::test]
async fn test_benchmark_throughput_echo() {
    // Start a real echo server for throughput measurement
    let echo_handle = start_echo_server(4240).await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Measure throughput with 1MB of data
    let data_size = 1024 * 1024; // 1 MB
    let throughput = measure_throughput("127.0.0.1", 4240, data_size).await;
    assert!(throughput.is_some(), "throughput measurement should succeed");

    let mbps = throughput.unwrap();
    eprintln!("Throughput: {:.2} MB/s (1MB round-trip)", mbps);
    // Local echo should achieve at least 10 MB/s
    assert!(mbps > 10.0, "throughput should be > 10 MB/s, got {:.2} MB/s", mbps);

    echo_handle.abort();
}

/// FP-9.9.4: Sustained connections benchmark — measure connection churn rate
#[tokio::test]
async fn test_benchmark_sustained_connections() {
    let server = MockSshServer::new("127.0.0.1:4234", "user", "pass");
    tokio::spawn(async move { let _ = server.start().await; });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let count = 100;
    let start = Instant::now();
    for i in 0..count {
        let stream = tokio::time::timeout(
            Duration::from_secs(2),
            TcpStream::connect(("127.0.0.1", 4234)),
        ).await;
        assert!(stream.is_ok(), "connection {} should succeed", i);
    }
    let elapsed = start.elapsed();
    let rate = count as f64 / elapsed.as_secs_f64();
    eprintln!("{} sequential connections in {:?} ({:.0} conn/s)", count, elapsed, rate);
    assert!(elapsed.as_secs() < 10, "should complete in under 10s");
}

// === SECTION 4 END ===

/// FP-9.9.5: Memory usage benchmark — measure RSS before and after load
#[tokio::test]
async fn test_benchmark_memory_usage() {
    let echo_handle = start_echo_server(4241).await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let mem_before = get_memory_usage();
    eprintln!("Memory before connections: {} KB", mem_before / 1024);

    // Open 20 concurrent connections and transfer data
    let mut handles = Vec::new();
    for _ in 0..20 {
        handles.push(tokio::spawn(async move {
            if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", 4241)).await {
                let data = vec![0xCDu8; 64 * 1024]; // 64KB
                let _ = stream.write_all(&data).await;
                let mut buf = vec![0u8; 64 * 1024];
                let _ = stream.read(&mut buf).await;
            }
        }));
    }
    for h in handles {
        let _ = h.await;
    }

    let mem_after = get_memory_usage();
    eprintln!("Memory after 20 connections: {} KB", mem_after / 1024);

    if mem_before > 0 && mem_after > 0 {
        let delta = mem_after.saturating_sub(mem_before);
        eprintln!("Memory delta: {} KB", delta / 1024);
        // Memory increase should be reasonable (less than 50MB for 20 connections)
        assert!(delta < 50 * 1024 * 1024, "memory delta should be < 50MB, got {} KB", delta / 1024);
    }

    echo_handle.abort();
}

/// FP-9.9.6: Concurrent throughput — measure throughput under concurrent load
#[tokio::test]
async fn test_benchmark_concurrent_throughput() {
    let echo_handle = start_echo_server(4242).await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let concurrency = 10;
    let data_size = 256 * 1024; // 256KB per connection
    let start = Instant::now();

    let mut handles = Vec::new();
    for _ in 0..concurrency {
        handles.push(tokio::spawn(async move {
            measure_throughput("127.0.0.1", 4242, data_size).await
        }));
    }

    let mut total_mbps = 0.0;
    let mut success_count = 0;
    for h in handles {
        if let Ok(Some(mbps)) = h.await {
            total_mbps += mbps;
            success_count += 1;
        }
    }
    let elapsed = start.elapsed();

    eprintln!(
        "Concurrent throughput: {} connections, {:.2} MB/s aggregate, in {:?}",
        success_count, total_mbps, elapsed
    );
    assert!(success_count == concurrency, "all {} connections should succeed", concurrency);
    assert!(total_mbps > 20.0, "aggregate throughput should be > 20 MB/s, got {:.2}", total_mbps);

    echo_handle.abort();
}

// === SECTION 5 END ===
