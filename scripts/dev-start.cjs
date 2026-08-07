// Cross-platform dev startup helper — kills stale processes and removes cargo build locks.
// Run before `tauri dev` to avoid "Blocking waiting for file lock" issues and
// stale daemon lock files that block embedded daemon startup.
//
// When called from `npm start` / `startwin` (before tauri dev starts), it
// kills ALL stale processes (termfast-app, cargo, rustc, port listeners).
// When called from `beforeDevCommand` (tauri dev is already starting), it
// only clears the WebView2 cache — killing cargo/rustc here would kill
// tauri dev's own build process.  However, it STILL kills stale
// termfast-app.exe instances because those hold the daemon lock and would
// prevent the new instance from starting its embedded daemon.
const { execSync } = require("child_process");
const fs = require("fs");
const path = require("path");

const isWindows = process.platform === "win32";

// Detect if tauri dev is already running (called from beforeDevCommand).
// In that case, our parent process tree includes cargo/tauri, so we must
// NOT kill cargo/rustc processes.
// Tauri 2 sets TAURI_ENV_PLATFORM and TAURI_ENV_DEBUG for beforeDevCommand.
const isBeforeDevCommand = process.env.TAURI_ENV_PLATFORM !== undefined || process.env.TAURI_ENV_DEBUG !== undefined;

function killStaleAppProcesses() {
  // Always kill stale termfast-app instances — even when called from
  // beforeDevCommand — because they hold the daemon lock and would prevent
  // the new instance from starting its embedded daemon.
  const appName = isWindows ? "termfast-app.exe" : "termfast-app";
  try {
    if (isWindows) {
      const out = execSync('tasklist /FI "IMAGENAME eq termfast-app.exe" /FO CSV /NH', { encoding: "utf8" });
      for (const line of out.trim().split("\n")) {
        const match = line.match(/"termfast-app\.exe","(\d+)"/);
        if (match) {
          const pid = match[1];
          try {
            execSync(`taskkill /PID ${pid} /F`, { stdio: "ignore" });
            console.log(`[dev-start] killed termfast-app.exe PID ${pid}`);
          } catch {}
        }
      }
    } else {
      // Unix: pkill termfast-app
      try {
        execSync(`pkill -f "termfast-app"`, { stdio: "ignore" });
        console.log("[dev-start] killed stale termfast-app processes");
      } catch {}
    }
  } catch {}
}

function killStaleProcesses() {
  // Always kill stale termfast-app instances (even in beforeDevCommand)
  killStaleAppProcesses();

  // Don't kill cargo/rustc when tauri dev is already running
  if (isBeforeDevCommand) return;

  if (isWindows) {
    // Kill stale cargo.exe processes
    try {
      const out = execSync('tasklist /FI "IMAGENAME eq cargo.exe" /FO CSV /NH', { encoding: "utf8" });
      for (const line of out.trim().split("\n")) {
        const match = line.match(/"cargo\.exe","(\d+)"/);
        if (match) {
          const pid = match[1];
          try {
            execSync(`taskkill /PID ${pid} /F`, { stdio: "ignore" });
            console.log(`[dev-start] killed cargo.exe PID ${pid}`);
          } catch {}
        }
      }
    } catch {}

    // Kill stale rustc.exe processes
    try {
      const out = execSync('tasklist /FI "IMAGENAME eq rustc.exe" /FO CSV /NH', { encoding: "utf8" });
      for (const line of out.trim().split("\n")) {
        const match = line.match(/"rustc\.exe","(\d+)"/);
        if (match) {
          const pid = match[1];
          try {
            execSync(`taskkill /PID ${pid} /F`, { stdio: "ignore" });
            console.log(`[dev-start] killed rustc.exe PID ${pid}`);
          } catch {}
        }
      }
    } catch {}
  } else {
    // Unix: kill stale cargo/rustc
    for (const proc of ["cargo", "rustc"]) {
      try {
        execSync(`pkill -f ${proc}`, { stdio: "ignore" });
        console.log(`[dev-start] killed stale ${proc} processes`);
      } catch {}
    }
  }
}

function killPortListeners() {
  if (isBeforeDevCommand) return; // Don't kill tauri dev's own Vite server

  for (const port of [1420, 1421]) {
    try {
      if (isWindows) {
        const out = execSync(`netstat -ano | findstr ":${port} "`, { encoding: "utf8" });
        const pids = new Set();
        for (const line of out.trim().split("\n")) {
          const parts = line.trim().split(/\s+/);
          const pid = parts[parts.length - 1];
          if (pid && /^\d+$/.test(pid)) pids.add(pid);
        }
        for (const pid of pids) {
          try {
            execSync(`taskkill /PID ${pid} /F`, { stdio: "ignore" });
            console.log(`[dev-start] killed port ${port} listener PID ${pid}`);
          } catch {}
        }
      } else {
        // Unix: lsof -ti :port
        try {
          const out = execSync(`lsof -ti :${port}`, { encoding: "utf8" }).trim();
          if (out) {
            execSync(`kill -9 ${out}`, { stdio: "ignore" });
            console.log(`[dev-start] killed port ${port} listeners`);
          }
        } catch {}
      }
    } catch {}
  }
}

function removeCargoLock() {
  if (isBeforeDevCommand) return; // Don't touch lock while cargo is running

  const targetDir = path.join(__dirname, "..", "target", "debug");
  const lockFile = path.join(targetDir, ".cargo-lock");
  if (fs.existsSync(lockFile)) {
    try {
      fs.unlinkSync(lockFile);
      console.log("[dev-start] removed stale .cargo-lock");
    } catch {}
  }
}

function clearWebviewCache() {
  const localAppData = process.env.LOCALAPPDATA;
  if (!localAppData) return;
  const cacheRoot = path.join(localAppData, "com.termfast.app", "EBWebView", "Default");
  const targets = [
    path.join(cacheRoot, "Cache", "Cache_Data"),
    path.join(cacheRoot, "Code Cache", "js"),
    path.join(cacheRoot, "Code Cache", "wasm"),
  ];
  for (const p of targets) {
    if (fs.existsSync(p)) {
      try {
        fs.rmSync(p, { recursive: true, force: true });
        console.log(`[dev-start] cleared ${path.basename(path.dirname(p))}/${path.basename(p)}`);
      } catch (e) {
        // Non-fatal — cache may be locked
      }
    }
  }
}

console.log("[dev-start] cleaning up stale processes and caches...");
killStaleProcesses();
killPortListeners();
removeCargoLock();
clearWebviewCache();
console.log("[dev-start] cleanup complete");
