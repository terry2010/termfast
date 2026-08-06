// Cross-platform port killer — kills processes listening on given ports.
// Usage: node scripts/kill-port.js 1420 1421
const { execSync } = require("child_process");

const ports = process.argv.slice(2).map(Number).filter(Boolean);

for (const port of ports) {
  try {
    if (process.platform === "win32") {
      // Windows: find PID via netstat, then taskkill
      const out = execSync(`netstat -ano | findstr ":${port} "`, { encoding: "utf8" });
      const pids = new Set();
      for (const line of out.trim().split("\n")) {
        const parts = line.trim().split(/\s+/);
        const pid = parts[parts.length - 1];
        if (pid && /^\d+$/.test(pid)) pids.add(pid);
      }
      for (const pid of pids) {
        try { execSync(`taskkill /PID ${pid} /F`, { stdio: "ignore" }); } catch {}
      }
    } else {
      // Unix (macOS/Linux): use lsof
      try { execSync(`lsof -ti:${port} | xargs kill -9`, { stdio: "ignore" }); } catch {}
    }
  } catch {}
}
