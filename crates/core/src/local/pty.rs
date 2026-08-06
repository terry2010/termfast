//! Local PTY — open a local shell in a pseudo-terminal.
//!
//! Wraps `portable-pty` (wezterm's cross-platform PTY library) to spawn
//! a local shell (zsh/bash/PowerShell/cmd) with proper terminal emulation.
//!
//! The returned `LocalPty` owns the reader/writer/master/child handles.
//! `killer` is a cloned `ChildKiller` for process cleanup independent of
//! the `child` handle (which may be moved into a task).

use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};

use super::shell::detect_default_shell;
#[cfg(unix)]
use super::shell::resolve_full_path;

/// An open local PTY session.
///
/// Fields are public because the daemon layer needs to move them into
/// separate `spawn_blocking` tasks (reader, writer) and the main task
/// (master for resize, child for kill).
pub struct LocalPty {
    /// PTY writer — moved into a `spawn_blocking` write task.
    pub writer: Box<dyn Write + Send>,
    /// PTY reader — moved into a `spawn_blocking` read task.
    pub reader: Box<dyn Read + Send>,
    /// Child process handle — used for kill on close / EOF.
    pub child: Box<dyn portable_pty::Child + Send + Sync>,
    /// Cloned killer — stored in `TerminalSession` for kill on close
    /// (independent of `child`, which lives in the main task).
    pub killer: Box<dyn portable_pty::ChildKiller + Send + Sync>,
    /// Master handle — used for resize (`set_size`).
    pub master: Box<dyn MasterPty + Send>,
}

/// Open a local PTY with the given dimensions and optional shell.
///
/// If `shell` is `None`, the system default shell is detected via
/// `detect_default_shell()`. On Unix, the shell is started with `-l`
/// (login mode) and a resolved PATH (see `resolve_full_path`).
pub fn open_local_pty(
    cols: u16,
    rows: u16,
    shell: Option<&str>,
) -> Result<LocalPty, Box<dyn std::error::Error>> {
    let pty_system = native_pty_system();
    let pair = pty_system.openpty(PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    })?;

    let shell = shell
        .map(|s| s.to_string())
        .unwrap_or_else(detect_default_shell);
    let mut cmd = CommandBuilder::new(&shell);

    // Terminal environment
    cmd.env("TERM", "xterm-256color");
    cmd.env("TERM_PROGRAM", "TermFast");

    #[cfg(unix)]
    {
        cmd.env("HOME", std::env::var("HOME").unwrap_or_default());
        cmd.env("USER", std::env::var("USER").unwrap_or_default());
        cmd.env(
            "LANG",
            std::env::var("LANG").unwrap_or_else(|_| "en_US.UTF-8".into()),
        );
        // Resolve full PATH (Tauri from Dock may have incomplete PATH)
        if let Ok(full_path) = resolve_full_path() {
            cmd.env("PATH", full_path);
        }
        // Login shell
        if shell.contains("zsh") || shell.contains("bash") {
            cmd.arg("-l");
        }
    }

    let child = pair.slave.spawn_command(cmd)?;
    // Close slave so the PTY can detect process exit (EOF on master)
    drop(pair.slave);
    // Clone killer after spawn (must be after spawn_command)
    let killer = child.clone_killer();
    let reader = pair.master.try_clone_reader()?;
    // take_writer takes &self, so master is still valid for resize after this call.
    // No need to clone master — just move it into the struct.
    let mut writer = pair.master.take_writer()?;

    // Windows ConPTY (portable-pty 0.9) creates the pseudoconsole with the
    // PSEUDOCONSOLE_INHERIT_CURSOR flag. On init, ConPTY emits a Device Status
    // Report request (DSR, `\x1b[6n`) on the output pipe and then BLOCKS until
    // the host responds with a Cursor Position Report (CPR, `\x1b[<row>;<col>R`)
    // on the input pipe. If no response is written, ConPTY hangs indefinitely
    // and the reader never produces any output — the terminal appears dead.
    //
    // Write a CPR (row 1, col 1) preemptively to unblock ConPTY. The data sits
    // in the pipe buffer and ConPTY reads it when ready. Verified working with
    // both cmd.exe and pwsh (PowerShell 7) — first keystroke is NOT consumed.
    // The frontend (xterm.js) also handles DSR responses as a secondary path.
    // See: https://github.com/vercel/turborepo/pull/11816
    #[cfg(target_os = "windows")]
    {
        let _ = writer.write_all(b"\x1b[1;1R");
        let _ = writer.flush();
    }

    Ok(LocalPty {
        writer,
        reader,
        child,
        killer,
        master: pair.master,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn test_open_local_pty() {
        let mut pty = open_local_pty(80, 24, None).unwrap();
        pty.writer.write_all(b"echo hello_termfast\n").unwrap();
        pty.writer.flush().unwrap();
        // Poll for output (shell startup time varies; .zshrc may take 500ms+)
        let mut output = String::new();
        let mut buf = vec![0u8; 4096];
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            match pty.reader.read(&mut buf) {
                Ok(n) if n > 0 => {
                    output.push_str(&String::from_utf8_lossy(&buf[..n]));
                    if output.contains("hello_termfast") {
                        break;
                    }
                }
                _ => std::thread::sleep(Duration::from_millis(50)),
            }
        }
        // Strip ANSI escape sequences (e.g. \x1b[93m) so we can check the
        // plain text content. PowerShell's PSReadLine wraps echoed commands
        // in color codes, so raw output may look like "\x1b[93mecho\x1b[m hello".
        let plain = strip_ansi(&output);
        // Assert the full "echo hello_termfast" appears in the stripped output.
        // This verifies the first character 'e' was NOT consumed by the
        // preemptive CPR response (if it were, the echo would show
        // "cho hello_termfast" instead).
        assert!(
            plain.contains("echo hello_termfast"),
            "expected 'echo hello_termfast' in stripped output (first char not eaten), got: {}",
            plain
        );
        let _ = pty.child.kill();
    }

    /// Strip ANSI escape sequences (CSI ... m and similar) from a string.
    fn strip_ansi(s: &str) -> String {
        let mut result = String::with_capacity(s.len());
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                // Skip ESC [ ... <final byte 0x40-0x7e>
                if chars.peek() == Some(&'[') {
                    chars.next();
                    while let Some(&c2) = chars.peek() {
                        chars.next();
                        if c2 as u32 >= 0x40 && c2 as u32 <= 0x7e {
                            break;
                        }
                    }
                    continue;
                }
                // Skip ESC ] ... BEL (OSC sequences)
                if chars.peek() == Some(&']') {
                    chars.next();
                    while let Some(c2) = chars.next() {
                        if c2 == '\x07' {
                            break;
                        }
                    }
                    continue;
                }
            }
            result.push(c);
        }
        result
    }

    #[test]
    fn test_resize() {
        let mut pty = open_local_pty(80, 24, None).unwrap();
        // Resize to 120x40
        pty.master
            .resize(PtySize {
                rows: 40,
                cols: 120,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        // Verify resize took effect.
        // On Unix, `stty size` outputs "rows cols" (e.g. "40 120").
        // On Windows, `$host.UI.RawUI.WindowSize` outputs the size, but
        // the simplest cross-platform check is to use the master's get_size()
        // which queries the PTY directly without relying on shell commands.
        #[cfg(unix)]
        {
            pty.writer.write_all(b"stty size\n").unwrap();
            pty.writer.flush().unwrap();
            let mut output = String::new();
            let mut buf = vec![0u8; 4096];
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline {
                match pty.reader.read(&mut buf) {
                    Ok(n) if n > 0 => {
                        output.push_str(&String::from_utf8_lossy(&buf[..n]));
                        if output.contains("40 120") {
                            break;
                        }
                    }
                    _ => std::thread::sleep(Duration::from_millis(50)),
                }
            }
            assert!(output.contains("40 120"), "resize output was: {}", output);
        }
        #[cfg(windows)]
        {
            // On Windows, verify via the master's get_size() API
            let size = pty.master.get_size().unwrap();
            assert_eq!(size.rows, 40, "resize rows mismatch");
            assert_eq!(size.cols, 120, "resize cols mismatch");
        }
        let _ = pty.child.kill();
    }
}
