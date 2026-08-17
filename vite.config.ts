import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import path from "path";

const host = process.env.TAURI_DEV_HOST;

// Custom plugin to send no-cache headers for all dev server responses.
// This prevents WebView2 (Tauri's browser engine on Windows) from caching
// JS modules, which would mask HMR updates.
const noCachePlugin = () => ({
  name: "no-cache-headers",
  configureServer(server: any) {
    server.middlewares.use((_req: any, res: any, next: any) => {
      res.setHeader("Cache-Control", "no-store, no-cache, must-revalidate");
      res.setHeader("Pragma", "no-cache");
      next();
    });
  },
});

// Exclude docs/ directory from vite's dependency scanning and compilation.
// docs/ contains third-party reference code (opencode, cli-agent-orchestrator)
// that has its own dependencies (solid-js, etc.) not installed in this project.
const excludeDocsPlugin = () => ({
  name: "exclude-docs",
  resolveId(source: string, importer: string | undefined) {
    if (importer && importer.includes("/docs/")) {
      return { id: source, external: true };
    }
    return null;
  },
});

export default defineConfig({
  plugins: [react(), tailwindcss(), noCachePlugin(), excludeDocsPlugin()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || true,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : {
          protocol: "ws",
          host: "localhost",
          port: 1421,
        },
    watch: {
      ignored: ["**/src-tauri/**", "**/docs/**", "**/android/**", "**/crates/**", "**/server/**", "**/scripts/**"],
    },
  },
  test: {
    globals: true,
    environment: "jsdom",
    setupFiles: "./src/test/setup.ts",
    exclude: [
      "**/node_modules/**",
      "**/dist/**",
      "e2e",
      "src-tauri",
      "website",
      "docs/**",
    ],
  },
});
