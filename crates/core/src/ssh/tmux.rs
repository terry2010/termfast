//! tmux session management — Phase 1 (Release 1 + Release 2)
//!
//! Detect tmux on remote server, list/create/attach TermFast-tagged sessions.
//! Shared between desktop (Release 1) and Android (Release 2).
//!
//! Uses tmux user options (`@termfast*`) to mark TermFast-created sessions,
//! independent of session name prefix — users can `tmux rename-session` freely.

use crate::error::{Error, Result};
use crate::ssh::exec;
use russh::client;

/// TermFast-created tmux session metadata (parsed from `tmux list-sessions -F`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxSession {
    pub name: String,
    pub description: String,       // @termfast_description
    pub created: u64,              // @termfast_created (unix timestamp)
    pub server: String,            // @termfast_server
    pub size: (u16, u16),          // @termfast_size (cols, rows)
    pub windows: u32,              // session_windows
    pub attached_count: u32,       // session_attached
    pub last_activity: u64,        // session_activity (unix timestamp)
}

/// User's tmux mode preference
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TmuxMode {
    /// Auto-restore the most recently active session (no popup)
    Auto,
    /// Ask user via popup (default)
    #[default]
    Ask,
    /// Always create a new session
    AlwaysNew,
    /// Never use tmux
    Disabled,
}

impl TmuxMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            TmuxMode::Auto => "auto",
            TmuxMode::Ask => "ask",
            TmuxMode::AlwaysNew => "always_new",
            TmuxMode::Disabled => "disabled",
        }
    }

    pub fn parse_str(s: &str) -> Self {
        match s {
            "auto" => TmuxMode::Auto,
            "always_new" => TmuxMode::AlwaysNew,
            "disabled" => TmuxMode::Disabled,
            _ => TmuxMode::Ask, // default + unknown fallback
        }
    }
}

/// User's choice when presented with tmux session options
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TmuxChoice {
    /// Attach to an existing session
    Attach(String),
    /// Create a new session with optional description
    New { description: String, server_name: String },
    /// Don't use tmux, just open a normal shell
    None,
}

// === SECTION 1 END ===

/// Detect whether tmux is installed on the remote server.
/// Returns false if tmux is not found or detection times out (3s).
pub async fn detect_tmux(handle: &client::Handle<super::client::SshHandler>) -> bool {
    match exec(handle, "command -v tmux", 3).await {
        Ok(result) => result.is_success() && !result.stdout.trim().is_empty(),
        Err(_) => false,
    }
}

/// List all TermFast-tagged tmux sessions on the remote server.
/// Returns empty Vec if tmux is not installed or no @termfast sessions exist.
///
/// Format string uses `|` as field separator. Fields:
/// `#{session_name}|#{@termfast}|#{@termfast_description}|#{@termfast_created}|#{@termfast_server}|#{@termfast_size}|#{session_created}|#{session_windows}|#{session_attached}|#{session_activity}`
pub async fn list_termfast_sessions(
    handle: &client::Handle<super::client::SshHandler>,
) -> Result<Vec<TmuxSession>> {
    let fmt = "#{session_name}|#{@termfast}|#{@termfast_description}|#{@termfast_created}|#{@termfast_server}|#{@termfast_size}|#{session_created}|#{session_windows}|#{session_attached}|#{session_activity}";
    let cmd = format!("tmux list-sessions -F '{}' 2>/dev/null", fmt);
    let result = exec(handle, &cmd, 5).await?;

    if !result.is_success() {
        // tmux not installed or no sessions — return empty
        return Ok(Vec::new());
    }

    let mut sessions = Vec::new();
    for line in result.stdout.lines() {
        if let Some(session) = parse_session_line(line) {
            sessions.push(session);
        }
    }
    // Sort by last_activity descending (most recent first)
    sessions.sort_by_key(|s| std::cmp::Reverse(s.last_activity));
    Ok(sessions)
}

/// Parse a single line from `tmux list-sessions -F` output.
/// Returns None if the line doesn't have @termfast == "true".
fn parse_session_line(line: &str) -> Option<TmuxSession> {
    let fields: Vec<&str> = line.split('|').collect();
    if fields.len() < 10 {
        return None;
    }

    // Only include sessions marked with @termfast == "true"
    if fields[1] != "true" {
        return None;
    }

    let name = fields[0].to_string();
    let description = fields[2].to_string();
    // @termfast_created may be empty (old sessions) — fallback to session_created
    let created = fields[3].parse::<u64>().ok()
        .or_else(|| fields[6].parse::<u64>().ok())
        .unwrap_or(0);
    let server = fields[4].to_string();
    let size = parse_size(fields[5]);
    let windows = fields[7].parse::<u32>().ok().unwrap_or(0);
    let attached_count = fields[8].parse::<u32>().ok().unwrap_or(0);
    let last_activity = fields[9].parse::<u64>().ok().unwrap_or(0);

    Some(TmuxSession {
        name,
        description,
        created,
        server,
        size,
        windows,
        attached_count,
        last_activity,
    })
}

/// Parse "colsxrows" format (e.g. "120x40") → (cols, rows).
/// Returns (80, 24) as fallback if parsing fails.
fn parse_size(s: &str) -> (u16, u16) {
    let s = s.trim();
    if let Some(idx) = s.find('x') {
        let cols = s[..idx].parse::<u16>().ok().unwrap_or(80);
        let rows = s[idx + 1..].parse::<u16>().ok().unwrap_or(24);
        (cols, rows)
    } else {
        (80, 24)
    }
}

// === SECTION 2 END ===

/// Check if a tmux session name already exists.
/// Uses `tmux has-session -t <name>` (exit 0 = exists, non-zero = not exists).
/// Appends `; echo "EXIT:$?"` as a fallback in case the SSH channel doesn't
/// deliver the exit status reliably.
pub async fn session_exists(
    handle: &client::Handle<super::client::SshHandler>,
    name: &str,
) -> Result<bool> {
    // Use `; echo "EXIT:$?"` to capture tmux's exit code in stdout,
    // because the exec channel returns the exit code of the LAST command
    // (echo), which is always 0.
    let cmd = format!(
        "tmux has-session -t {} 2>/dev/null; echo \"EXIT:$?\"",
        shell_escape(name)
    );
    let result = exec(handle, &cmd, 3).await?;
    // Parse stdout for "EXIT:N" marker — this is the only reliable way
    // to get tmux's exit code when using the echo fallback.
    if let Some(pos) = result.stdout.rfind("EXIT:") {
        let code_str = &result.stdout[pos + 5..].trim();
        if let Ok(code) = code_str.parse::<u32>() {
            return Ok(code == 0);
        }
    }
    // Fallback: if no EXIT marker found, use exec channel exit code
    Ok(result.exit_code == 0)
}

/// Generate a unique tmux session name: `termfast_<timestamp>_<random8hex>`.
/// Checks for collisions via `tmux has-session`, retries up to 3 times.
/// Returns Err if all 3 attempts collide (extremely unlikely).
pub async fn generate_unique_session_name(
    handle: &client::Handle<super::client::SshHandler>,
) -> Result<String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    for _ in 0..3 {
        let rand_hex: String = (0..4)
            .map(|_| format!("{:02x}", rand::random::<u8>()))
            .collect();
        let name = format!("termfast_{}_{}", now, rand_hex);
        if !session_exists(handle, &name).await? {
            return Ok(name);
        }
    }
    Err(Error::Other(
        "tmux session name collision after 3 attempts".to_string(),
    ))
}

/// Build the `tmux attach -t <name>` command string (with newline for PTY injection).
/// Uses window-size=manual so each client can resize to its own dimensions
/// on attach (via resize-window). The last client to attach gets full view.
/// Enables allow-passthrough for ZModem (rz/sz) support through tmux (3.3+).
pub fn build_attach_command(session_name: &str) -> String {
    let name = shell_escape(session_name);
    format!(
        "tmux set-option -t {name} window-size manual 2>/dev/null; \
tmux set-option -t {name} allow-passthrough on 2>/dev/null; \
tmux attach -t {name}\n"
    )
}

/// Build a batched exec command to create + configure a new tmux session.
/// All commands chained with `&&` for single exec (1 RTT).
/// Returns the shell command string (no newline — executed via exec channel, not PTY).
pub fn build_new_session_exec_command(
    session_name: &str,
    description: &str,
    server_name: &str,
    cols: u16,
    rows: u16,
) -> String {
    let name = shell_escape(session_name);
    let desc = shell_escape(description);
    let srv = shell_escape(server_name);
    let size = format!("{}x{}", cols, rows);
    // tmux new -d creates a detached session (no client attached)
    // set-option -t <name> sets user options on the session
    // window-size=manual: each client resizes to its own dimensions on attach.
    // The last client to attach/resize gets the full view.
    // allow-passthrough: let ZModem (rz/sz) control sequences pass through tmux
    // to the client (tmux 3.3+). Wrapped in 2>/dev/null for older tmux versions.
    format!(
        "tmux new -s {name} -d \
&& tmux set-option -t {name} @termfast true \
&& tmux set-option -t {name} @termfast_description {desc} \
&& tmux set-option -t {name} @termfast_created \"$(date +%s)\" \
&& tmux set-option -t {name} @termfast_server {srv} \
&& tmux set-option -t {name} @termfast_size {size} \
&& tmux set-option -t {name} window-size manual \
&& tmux set-option -t {name} allow-passthrough on 2>/dev/null"
    )
}

/// Build the command to resize a tmux window and update @termfast_size metadata.
/// Uses resize-window to actually change the tmux rendering size (needed when
/// window-size=manual). Called on PTY resize from any client.
pub fn build_update_size_command(session_name: &str, cols: u16, rows: u16) -> String {
    let name = shell_escape(session_name);
    format!(
        "tmux resize-window -t {name} -x {cols} -y {rows} 2>/dev/null; \
tmux set-option -t {name} @termfast_size \"{cols}x{rows}\""
    )
}

/// Build the `tmux kill-session -t <name>` command for explicit session termination.
pub fn build_kill_session_command(session_name: &str) -> String {
    format!("tmux kill-session -t {}", shell_escape(session_name))
}

/// Escape a string for safe use in a shell command argument.
/// Wraps in single quotes and escapes any embedded single quotes.
pub fn shell_escape(s: &str) -> String {
    // Replace ' with '\'' (close quote, escaped quote, reopen quote)
    let escaped = s.replace('\'', "'\\''");
    format!("'{}'", escaped)
}

// === SECTION 3 END ===

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_session_line_valid() {
        let line = "termfast_abc|true|my dev session|1700000000|my-server|120x40|1700000000|3|1|1700001000";
        let session = parse_session_line(line).unwrap();
        assert_eq!(session.name, "termfast_abc");
        assert_eq!(session.description, "my dev session");
        assert_eq!(session.created, 1700000000);
        assert_eq!(session.server, "my-server");
        assert_eq!(session.size, (120, 40));
        assert_eq!(session.windows, 3);
        assert_eq!(session.attached_count, 1);
        assert_eq!(session.last_activity, 1700001000);
    }

    #[test]
    fn test_parse_session_line_not_termfast() {
        let line = "user_session|false||0||80x24|1700000000|1|0|1700000500";
        assert!(parse_session_line(line).is_none());
    }

    #[test]
    fn test_parse_session_line_too_few_fields() {
        let line = "termfast_abc|true|desc";
        assert!(parse_session_line(line).is_none());
    }

    #[test]
    fn test_parse_session_line_empty_created_falls_back() {
        // @termfast_created empty → fallback to session_created (field 6)
        let line = "termfast_x|true|desc||srv|80x24|1700000050|1|0|1700000100";
        let session = parse_session_line(line).unwrap();
        assert_eq!(session.created, 1700000050);
    }

    #[test]
    fn test_parse_session_line_invalid_numbers() {
        let line = "termfast_x|true|desc|notanumber|srv|bad|alsonot|bad|bad|bad";
        let session = parse_session_line(line).unwrap();
        assert_eq!(session.created, 0);
        assert_eq!(session.size, (80, 24));
        assert_eq!(session.windows, 0);
    }

    #[test]
    fn test_parse_size_valid() {
        assert_eq!(parse_size("120x40"), (120, 40));
        assert_eq!(parse_size("80x24"), (80, 24));
        assert_eq!(parse_size(" 200x50 "), (200, 50));
    }

    #[test]
    fn test_parse_size_invalid() {
        assert_eq!(parse_size("invalid"), (80, 24));
        assert_eq!(parse_size(""), (80, 24));
        assert_eq!(parse_size("120"), (80, 24));
    }

    #[test]
    fn test_shell_escape_simple() {
        assert_eq!(shell_escape("hello"), "'hello'");
    }

    #[test]
    fn test_shell_escape_with_single_quote() {
        // ' inside → close quote, escaped quote, reopen
        assert_eq!(shell_escape("it's"), "'it'\\''s'");
    }

    #[test]
    fn test_shell_escape_empty() {
        assert_eq!(shell_escape(""), "''");
    }

    #[test]
    fn test_build_attach_command() {
        let cmd = build_attach_command("termfast_abc");
        assert!(cmd.contains("tmux attach -t 'termfast_abc'"));
        assert!(cmd.contains("window-size manual"));
        assert!(cmd.contains("allow-passthrough on"));
    }

    #[test]
    fn test_build_attach_command_escaped() {
        let cmd = build_attach_command("name with space");
        assert!(cmd.contains("tmux attach -t 'name with space'"));
        assert!(cmd.contains("window-size manual"));
        assert!(cmd.contains("allow-passthrough on"));
    }

    #[test]
    fn test_build_new_session_exec_command() {
        let cmd = build_new_session_exec_command("termfast_x", "my desc", "srv1", 120, 40);
        assert!(cmd.contains("tmux new -s 'termfast_x' -d"));
        assert!(cmd.contains("@termfast true"));
        assert!(cmd.contains("@termfast_description 'my desc'"));
        assert!(cmd.contains("@termfast_server 'srv1'"));
        assert!(cmd.contains("@termfast_size 120x40"));
        assert!(cmd.contains("window-size manual"));
        assert!(cmd.contains("allow-passthrough"));
        assert!(cmd.contains("&&"));
    }

    #[test]
    fn test_build_update_size_command() {
        let cmd = build_update_size_command("termfast_x", 100, 30);
        assert!(cmd.contains("tmux resize-window -t 'termfast_x' -x 100 -y 30"));
        assert!(cmd.contains("@termfast_size \"100x30\""));
    }

    #[test]
    fn test_build_kill_session_command() {
        let cmd = build_kill_session_command("termfast_x");
        assert_eq!(cmd, "tmux kill-session -t 'termfast_x'");
    }

    #[test]
    fn test_tmux_mode_from_str() {
        assert_eq!(TmuxMode::parse_str("auto"), TmuxMode::Auto);
        assert_eq!(TmuxMode::parse_str("ask"), TmuxMode::Ask);
        assert_eq!(TmuxMode::parse_str("always_new"), TmuxMode::AlwaysNew);
        assert_eq!(TmuxMode::parse_str("disabled"), TmuxMode::Disabled);
        assert_eq!(TmuxMode::parse_str("unknown"), TmuxMode::Ask);
        assert_eq!(TmuxMode::parse_str(""), TmuxMode::Ask);
    }

    #[test]
    fn test_tmux_mode_as_str() {
        assert_eq!(TmuxMode::Auto.as_str(), "auto");
        assert_eq!(TmuxMode::Ask.as_str(), "ask");
        assert_eq!(TmuxMode::AlwaysNew.as_str(), "always_new");
        assert_eq!(TmuxMode::Disabled.as_str(), "disabled");
    }

    #[test]
    fn test_tmux_mode_default() {
        assert_eq!(TmuxMode::default(), TmuxMode::Ask);
    }

    #[test]
    fn test_tmux_mode_serde() {
        let json = serde_json::to_string(&TmuxMode::AlwaysNew).unwrap();
        assert_eq!(json, "\"always_new\"");
        let mode: TmuxMode = serde_json::from_str("\"disabled\"").unwrap();
        assert_eq!(mode, TmuxMode::Disabled);
    }
}
