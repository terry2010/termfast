// Mock SSH server for E2E tests — FP-9.1
// Starts a minimal SSH server that supports password auth and exec
// Used by Playwright E2E tests to verify SSH connectivity

import { Server } from "ssh2";
import { readFileSync } from "fs";
import { writeFileSync, existsSync } from "fs";
import { join } from "path";
import { tmpdir } from "os";

// Generate a host key if it doesn't exist
const keyPath = join(tmpdir(), "vps-guard-test-hostkey");
if (!existsSync(keyPath)) {
  const { generateKeyPairSync } = require("crypto");
  const { privateKey } = generateKeyPairSync("rsa", {
    modulusLength: 2048,
    privateKeyEncoding: { type: "pkcs1", format: "pem" },
  });
  writeFileSync(keyPath, privateKey);
}

const hostKey = readFileSync(keyPath, "utf-8");

const server = new Server(
  {
    hostKeys: [hostKey],
  },
  (client) => {
    client.on("authentication", (ctx) => {
      if (ctx.method === "password" && ctx.username === "testuser" && ctx.password === "testpass") {
        ctx.accept();
      } else {
        ctx.reject();
      }
    });

    client.on("ready", () => {
      client.on("session", (accept, reject) => {
        const session = accept();
        session.on("exec", (accept, reject, info) => {
          const stream = accept();
          const cmd = info.command;
          // Handle common test commands
          let output = "mock output\n";
          if (cmd.includes("echo $SSH_CONNECTION")) {
            output = "1.2.3.4 12345 5.6.7.8 22\n";
          } else if (cmd.includes("pgrep")) {
            output = "12345\n";
          } else if (cmd.includes("echo hello")) {
            output = "hello\n";
          } else if (cmd.includes("echo alive")) {
            output = "alive\n";
          }
          stream.write(output);
          stream.exit(0);
          stream.end();
        });
      });
    });

    client.on("end", () => {});
  }
);

const PORT = parseInt(process.env.MOCK_SSH_PORT || "2222");
server.listen(PORT, "127.0.0.1", () => {
  console.log(`Mock SSH server listening on 127.0.0.1:${PORT}`);
});
