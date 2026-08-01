//! Shell detection + PATH resolution.
//!
//! `detect_default_shell()` finds the user's default shell.
//! `resolve_full_path()` returns a complete PATH — on macOS this runs
//! `zsh -l -c 'echo $PATH'` to get the login-shell PATH, fixing the
//! incomplete PATH that Tauri apps get when launched from the Dock.

/// Detect the system default shell.
pub fn detect_default_shell() -> String {
    #[cfg(target_os = "windows")]
    {
        if which::which("pwsh").is_ok() {
            "pwsh".into()
        } else if which::which("powershell").is_ok() {
            "powershell".into()
        } else {
            "cmd.exe".into()
        }
    }
    #[cfg(unix)]
    {
        std::env::var("SHELL").unwrap_or_else(|_| {
            if which::which("zsh").is_ok() {
                "/bin/zsh".into()
            } else {
                "/bin/bash".into()
            }
        })
    }
}

/// List shells available on this system.
/// Returns shell names (not full paths), e.g. ["zsh", "bash"] on macOS.
pub fn list_available_shells() -> Vec<String> {
    let mut shells: Vec<String> = Vec::new();
    #[cfg(target_os = "windows")]
    {
        if which::which("pwsh").is_ok() {
            shells.push("pwsh".into());
        }
        if which::which("powershell").is_ok() {
            shells.push("powershell".into());
        }
        shells.push("cmd.exe".into());
    }
    #[cfg(not(target_os = "windows"))]
    {
        if which::which("zsh").is_ok() {
            shells.push("zsh".into());
        }
        if which::which("bash").is_ok() {
            shells.push("bash".into());
        }
    }
    shells
}

/// Resolve the full user PATH.
///
/// On macOS, Tauri apps launched from the Dock get a minimal PATH
/// (e.g. `/usr/bin:/bin`). This function runs a login shell to get the
/// complete PATH (including `/opt/homebrew/bin`, `~/.cargo/bin`, etc.).
/// Falls back to reading `/etc/paths` + `/etc/paths.d/*` if the login
/// shell returns empty.
#[cfg(target_os = "macos")]
pub fn resolve_full_path() -> Result<String, Box<dyn std::error::Error>> {
    use std::process::Command;
    let output = Command::new("/bin/zsh")
        .args(["-l", "-c", "echo $PATH"])
        .output()?;
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        // Fallback: read /etc/paths + /etc/paths.d/*
        let mut entries: Vec<String> = Vec::new();
        if let Ok(content) = std::fs::read_to_string("/etc/paths") {
            entries.extend(
                content
                    .lines()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty()),
            );
        }
        if let Ok(dirs) = std::fs::read_dir("/etc/paths.d") {
            for dir in dirs.flatten() {
                if let Ok(content) = std::fs::read_to_string(dir.path()) {
                    entries.extend(
                        content
                            .lines()
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty()),
                    );
                }
            }
        }
        entries.push("/usr/local/bin".into());
        entries.push("/opt/homebrew/bin".into());
        Ok(entries.join(":"))
    } else {
        Ok(path)
    }
}

#[cfg(target_os = "linux")]
pub fn resolve_full_path() -> Result<String, Box<dyn std::error::Error>> {
    Ok(std::env::var("PATH")?)
}

#[cfg(target_os = "android")]
pub fn resolve_full_path() -> Result<String, Box<dyn std::error::Error>> {
    Ok(std::env::var("PATH").unwrap_or_else(|_| "/system/bin:/system/xbin".into()))
}

#[cfg(target_os = "windows")]
pub fn resolve_full_path() -> Result<String, Box<dyn std::error::Error>> {
    Ok(std::env::var("PATH")?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_default_shell() {
        let shell = detect_default_shell();
        assert!(!shell.is_empty());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_resolve_full_path_macos() {
        let path = resolve_full_path().unwrap();
        assert!(!path.is_empty());
        assert!(path.contains("/bin"), "path was: {}", path);
        // PATH must include homebrew or /usr/local/bin
        assert!(
            path.contains("/opt/homebrew/bin") || path.contains("/usr/local/bin"),
            "path missing homebrew/local bin: {}",
            path
        );
    }
}
