//! Logging system — FP-6.7
//!
//! In-memory ring buffer for GUI display + optional file logging.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Log level
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

/// Log entry kind (for filtering)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogKind {
    Connection,
    Proxy,
    Trigger,
    Error,
    System,
}

/// A single log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: DateTime<Utc>,
    pub server_id: Option<String>,
    pub level: LogLevel,
    pub kind: LogKind,
    pub message: String,
    pub data: Option<serde_json::Value>,
    /// Optional execution_id for grouping trigger execution logs (FP-6.7)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_id: Option<String>,
}

/// In-memory log ring buffer
pub struct LogBuffer {
    entries: Arc<Mutex<VecDeque<LogEntry>>>,
    max_entries: usize,
    subscribers: Arc<Mutex<Vec<tokio::sync::mpsc::UnboundedSender<LogEntry>>>>,
}

impl LogBuffer {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: Arc::new(Mutex::new(VecDeque::with_capacity(max_entries))),
            max_entries,
            subscribers: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Add a log entry
    pub async fn add(&self, entry: LogEntry) {
        let mut entries = self.entries.lock().await;
        if entries.len() >= self.max_entries {
            entries.pop_front();
        }
        entries.push_back(entry.clone());
        drop(entries);

        // Notify subscribers
        let subs = self.subscribers.lock().await;
        for tx in subs.iter() {
            let _ = tx.send(entry.clone());
        }
    }

    /// Get all entries (filtered by optional criteria)
    pub async fn get_entries(
        &self,
        server_id: Option<&str>,
        kind: Option<&LogKind>,
        level: Option<&LogLevel>,
    ) -> Vec<LogEntry> {
        let entries = self.entries.lock().await;
        entries
            .iter()
            .filter(|e| {
                if let Some(sid) = server_id {
                    if e.server_id.as_deref() != Some(sid) {
                        return false;
                    }
                }
                if let Some(k) = kind {
                    if &e.kind != k {
                        return false;
                    }
                }
                if let Some(l) = level {
                    if &e.level != l {
                        return false;
                    }
                }
                true
            })
            .cloned()
            .collect()
    }

    /// Clear all entries
    pub async fn clear(&self) {
        self.entries.lock().await.clear();
    }

    /// Subscribe to new log entries
    pub async fn subscribe(&self) -> tokio::sync::mpsc::UnboundedReceiver<LogEntry> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        self.subscribers.lock().await.push(tx);
        rx
    }

    /// Get entry count
    pub async fn len(&self) -> usize {
        self.entries.lock().await.len()
    }

    /// Check if buffer is empty
    pub async fn is_empty(&self) -> bool {
        self.entries.lock().await.is_empty()
    }

    /// Search entries by regex pattern (FP-6.7)
    /// Searches message field. Returns entries matching the pattern.
    pub async fn search(&self, pattern: &str) -> Result<Vec<LogEntry>, regex::Error> {
        let re = regex::Regex::new(pattern)?;
        let entries = self.entries.lock().await;
        Ok(entries
            .iter()
            .filter(|e| re.is_match(&e.message))
            .cloned()
            .collect())
    }

    /// Search entries by regex with filters (server_id, kind, level)
    pub async fn search_filtered(
        &self,
        pattern: &str,
        server_id: Option<&str>,
        kind: Option<&LogKind>,
        level: Option<&LogLevel>,
    ) -> Result<Vec<LogEntry>, regex::Error> {
        let re = regex::Regex::new(pattern)?;
        let entries = self.entries.lock().await;
        Ok(entries
            .iter()
            .filter(|e| {
                if !re.is_match(&e.message) {
                    return false;
                }
                if let Some(sid) = server_id {
                    if e.server_id.as_deref() != Some(sid) {
                        return false;
                    }
                }
                if let Some(k) = kind {
                    if &e.kind != k {
                        return false;
                    }
                }
                if let Some(l) = level {
                    if &e.level != l {
                        return false;
                    }
                }
                true
            })
            .cloned()
            .collect())
    }

    /// Get all entries grouped by execution_id (FP-6.7)
    /// Returns a map of execution_id -> entries, sorted by timestamp.
    /// Entries without execution_id are excluded.
    pub async fn get_by_execution_id(
        &self,
        execution_id: &str,
    ) -> Vec<LogEntry> {
        let entries = self.entries.lock().await;
        entries
            .iter()
            .filter(|e| e.execution_id.as_deref() == Some(execution_id))
            .cloned()
            .collect()
    }

    /// Get all distinct execution_ids (FP-6.7)
    pub async fn list_execution_ids(&self) -> Vec<String> {
        let entries = self.entries.lock().await;
        let mut ids: Vec<String> = entries
            .iter()
            .filter_map(|e| e.execution_id.clone())
            .collect();
        ids.sort();
        ids.dedup();
        ids
    }
}

/// Helper to create a log entry
pub fn log_entry(
    server_id: Option<&str>,
    level: LogLevel,
    kind: LogKind,
    message: impl Into<String>,
) -> LogEntry {
    LogEntry {
        timestamp: Utc::now(),
        server_id: server_id.map(|s| s.to_string()),
        level,
        kind,
        message: message.into(),
        data: None,
        execution_id: None,
    }
}

/// Helper to create a log entry with execution_id (for trigger execution grouping)
pub fn log_entry_with_exec(
    server_id: Option<&str>,
    level: LogLevel,
    kind: LogKind,
    message: impl Into<String>,
    execution_id: impl Into<String>,
) -> LogEntry {
    LogEntry {
        timestamp: Utc::now(),
        server_id: server_id.map(|s| s.to_string()),
        level,
        kind,
        message: message.into(),
        data: None,
        execution_id: Some(execution_id.into()),
    }
}

// === SECTION 1 END ===

/// File logger with rotation (FP-6.7)
/// Writes log entries to a file, rotating when the file reaches a max size.
/// Rotated files are named with a timestamp suffix.
pub struct FileLogger {
    log_dir: std::path::PathBuf,
    current_file: std::path::PathBuf,
    max_file_size: u64,
    max_files: usize,
    current_size: std::sync::atomic::AtomicU64,
}

impl FileLogger {
    /// Create a new file logger in the given directory.
    /// `max_file_size` is in bytes. `max_files` is the max number of rotated files to keep.
    pub fn new(
        log_dir: impl Into<std::path::PathBuf>,
        max_file_size: u64,
        max_files: usize,
    ) -> std::io::Result<Self> {
        let log_dir = log_dir.into();
        std::fs::create_dir_all(&log_dir)?;
        let current_file = log_dir.join("vps-guard.log");
        let current_size = match std::fs::metadata(&current_file) {
            Ok(meta) => meta.len(),
            Err(_) => 0,
        };
        Ok(Self {
            log_dir,
            current_file,
            max_file_size,
            max_files,
            current_size: std::sync::atomic::AtomicU64::new(current_size),
        })
    }

    /// Write a log entry to the file. Rotates if needed.
    pub fn write(&self, entry: &LogEntry) -> std::io::Result<()> {
        let line = format!(
            "[{}] [{:?}] [{:?}] {} {}\n",
            entry.timestamp.to_rfc3339(),
            entry.level,
            entry.kind,
            entry.server_id.as_deref().unwrap_or("-"),
            entry.message
        );
        let line_bytes = line.len() as u64;

        // Check if rotation is needed
        let current = self.current_size.load(std::sync::atomic::Ordering::Relaxed);
        if current + line_bytes > self.max_file_size {
            self.rotate()?;
            self.current_size
                .store(0, std::sync::atomic::Ordering::Relaxed);
        }

        // Append to current file
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.current_file)?;
        file.write_all(line.as_bytes())?;
        self.current_size
            .fetch_add(line_bytes, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }

    /// Rotate the current log file.
    /// Renames current file to `vps-guard-<timestamp>.log` and cleans up old files.
    fn rotate(&self) -> std::io::Result<()> {
        if !self.current_file.exists() {
            return Ok(());
        }

        let timestamp = chrono::Utc::now().format("%Y%m%dT%H%M%S");
        let rotated_name = format!("vps-guard-{}.log", timestamp);
        let rotated_path = self.log_dir.join(&rotated_name);
        std::fs::rename(&self.current_file, &rotated_path)?;

        // Clean up old rotated files (keep max_files)
        let mut rotated_files: Vec<(std::path::PathBuf, std::time::SystemTime)> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.log_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with("vps-guard-") && name.ends_with(".log") {
                        if let Ok(meta) = entry.metadata() {
                            let mtime = meta.modified().unwrap_or(std::time::SystemTime::now());
                            rotated_files.push((path, mtime));
                        }
                    }
                }
            }
        }

        // Sort by mtime descending (newest first)
        rotated_files.sort_by(|a, b| b.1.cmp(&a.1));

        // Remove files beyond max_files
        for (path, _) in rotated_files.iter().skip(self.max_files) {
            let _ = std::fs::remove_file(path);
        }

        Ok(())
    }

    /// Get the current log file path
    pub fn current_path(&self) -> &std::path::Path {
        &self.current_file
    }
}

// === SECTION 2 END ===

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_add_and_get_entries() {
        let buffer = LogBuffer::new(100);
        buffer
            .add(log_entry(Some("srv_1"), LogLevel::Info, LogKind::Connection, "connected"))
            .await;
        buffer
            .add(log_entry(Some("srv_2"), LogLevel::Error, LogKind::Error, "failed"))
            .await;

        let all = buffer.get_entries(None, None, None).await;
        assert_eq!(all.len(), 2);

        let srv1 = buffer.get_entries(Some("srv_1"), None, None).await;
        assert_eq!(srv1.len(), 1);

        let errors = buffer.get_entries(None, None, Some(&LogLevel::Error)).await;
        assert_eq!(errors.len(), 1);
    }

    #[tokio::test]
    async fn test_ring_buffer_eviction() {
        let buffer = LogBuffer::new(3);
        for i in 0..5 {
            buffer
                .add(log_entry(None, LogLevel::Info, LogKind::System, format!("entry {}", i)))
                .await;
        }
        let entries = buffer.get_entries(None, None, None).await;
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].message, "entry 2");
    }

    #[tokio::test]
    async fn test_clear() {
        let buffer = LogBuffer::new(100);
        buffer
            .add(log_entry(None, LogLevel::Info, LogKind::System, "test"))
            .await;
        assert_eq!(buffer.len().await, 1);
        buffer.clear().await;
        assert_eq!(buffer.len().await, 0);
    }

    #[tokio::test]
    async fn test_subscribe() {
        let buffer = LogBuffer::new(100);
        let mut rx = buffer.subscribe().await;
        buffer
            .add(log_entry(None, LogLevel::Info, LogKind::System, "test"))
            .await;
        let entry = rx.recv().await.unwrap();
        assert_eq!(entry.message, "test");
    }

    #[tokio::test]
    async fn test_regex_search() {
        let buffer = LogBuffer::new(100);
        buffer
            .add(log_entry(None, LogLevel::Info, LogKind::System, "connection established to 1.2.3.4"))
            .await;
        buffer
            .add(log_entry(None, LogLevel::Info, LogKind::System, "proxy started on port 1080"))
            .await;
        buffer
            .add(log_entry(None, LogLevel::Error, LogKind::Error, "connection refused"))
            .await;

        // Search for IP addresses
        let results = buffer.search(r"\d+\.\d+\.\d+\.\d+").await.unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].message.contains("1.2.3.4"));

        // Search for "connection"
        let results = buffer.search(r"connection").await.unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_execution_id_grouping() {
        let buffer = LogBuffer::new(100);
        buffer
            .add(log_entry_with_exec(
                Some("srv_1"),
                LogLevel::Info,
                LogKind::Trigger,
                "trigger fired",
                "exec_001",
            ))
            .await;
        buffer
            .add(log_entry_with_exec(
                Some("srv_1"),
                LogLevel::Info,
                LogKind::Trigger,
                "command 1 executed",
                "exec_001",
            ))
            .await;
        buffer
            .add(log_entry_with_exec(
                Some("srv_1"),
                LogLevel::Info,
                LogKind::Trigger,
                "trigger fired",
                "exec_002",
            ))
            .await;
        buffer
            .add(log_entry(None, LogLevel::Info, LogKind::System, "unrelated log"))
            .await;

        // Get entries for exec_001
        let exec1 = buffer.get_by_execution_id("exec_001").await;
        assert_eq!(exec1.len(), 2);

        // Get entries for exec_002
        let exec2 = buffer.get_by_execution_id("exec_002").await;
        assert_eq!(exec2.len(), 1);

        // List all execution IDs
        let ids = buffer.list_execution_ids().await;
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"exec_001".to_string()));
        assert!(ids.contains(&"exec_002".to_string()));
    }

    #[tokio::test]
    async fn test_file_logger() {
        let tmp = std::env::temp_dir().join(format!("vps-guard-test-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let logger = FileLogger::new(&tmp, 1024, 3).unwrap();

        let entry = log_entry(Some("srv_1"), LogLevel::Info, LogKind::System, "test message");
        logger.write(&entry).unwrap();

        let content = std::fs::read_to_string(logger.current_path()).unwrap();
        assert!(content.contains("test message"));

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[tokio::test]
    async fn test_file_logger_rotation() {
        let tmp = std::env::temp_dir().join(format!("vps-guard-test-log-rot-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        // Very small max size to trigger rotation
        let logger = FileLogger::new(&tmp, 100, 3).unwrap();

        // Write enough entries to trigger rotation
        for i in 0..20 {
            let entry = log_entry(None, LogLevel::Info, LogKind::System, format!("log entry {}", i));
            logger.write(&entry).unwrap();
        }

        // Check that rotated files exist
        let mut rotated_count = 0;
        if let Ok(entries) = std::fs::read_dir(&tmp) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    if name.starts_with("vps-guard-") {
                        rotated_count += 1;
                    }
                }
            }
        }
        assert!(rotated_count > 0, "expected at least one rotated file");

        std::fs::remove_dir_all(&tmp).ok();
    }
}
