// E2E test: Server list interactions (FP-9.3, U6, U7, U8)
// Tests: rendering, selection, port copy chip, inline proxy toggle,
// connect-all/disconnect-all, abnormal pinning, delete with confirm.

import { test, expect } from "@playwright/test";
import { mockTauri, waitForAppReady, getCallsFor, getMockStore } from "./fixtures";

// === SECTION 1 END ===

test.beforeEach(async ({ page }) => {
  await mockTauri(page);
});

test.describe("Server List rendering", () => {
  test("renders all server names from mock data", async ({ page }) => {
    await waitForAppReady(page);
    await expect(page.locator("text=Tokyo VPS")).toBeVisible({ timeout: 10000 });
    await expect(page.locator("text=US West")).toBeVisible({ timeout: 5000 });
  });

  test("shows host address under server name", async ({ page }) => {
    await waitForAppReady(page);
    await expect(page.locator("text=1.2.3.4")).toBeVisible({ timeout: 5000 });
    await expect(page.locator("text=5.6.7.8")).toBeVisible({ timeout: 5000 });
  });

});

test.describe("Server selection", () => {
  test("clicking a server highlights it and shows detail", async ({ page }) => {
    await waitForAppReady(page);
    await page.locator("text=Tokyo VPS").first().click();
    await page.waitForTimeout(300);
    // Detail panel should show the server name as header
    await expect(page.locator(".text-xl.font-bold:has-text('Tokyo VPS')")).toBeVisible({ timeout: 3000 });
  });

  test("selecting different server changes detail content", async ({ page }) => {
    await waitForAppReady(page);
    // Select first
    await page.locator("text=Tokyo VPS").first().click();
    await page.waitForTimeout(200);
    await expect(page.locator(".text-xl.font-bold:has-text('Tokyo VPS')")).toBeVisible({ timeout: 3000 });
    // Select second
    await page.locator("text=US West").first().click();
    await page.waitForTimeout(200);
    await expect(page.locator(".text-xl.font-bold:has-text('US West')")).toBeVisible({ timeout: 3000 });
  });
});

test.describe("Port copy chip (U8)", () => {
  test("port chip shows :PORT for each server", async ({ page }) => {
    await waitForAppReady(page);
    await expect(page.locator("button:has-text(':1080')")).toBeVisible({ timeout: 5000 });
    await expect(page.locator("button:has-text(':1081')")).toBeVisible({ timeout: 5000 });
  });

  test("clicking port chip shows copied indicator", async ({ page }) => {
    await waitForAppReady(page);
    // Grant clipboard permissions for headless browser
    await page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
    const portChip = page.locator("button:has-text(':1080')").first();
    await portChip.click();
    await page.waitForTimeout(300);
    // After click, should show ✓ (copied indicator) — use first to avoid strict mode
    await expect(page.locator("button").filter({ hasText: "✓" })).toBeVisible({ timeout: 3000 });
  });
});

test.describe("Inline proxy toggle (U6)", () => {
  test("inline toggle button exists in server list item", async ({ page }) => {
    await waitForAppReady(page);
    // The inline toggle has aria-label "Toggle proxy"
    await expect(page.locator("[aria-label='Toggle proxy']").first()).toBeVisible({ timeout: 5000 });
  });

  test("clicking inline toggle calls ipc_toggle_proxy", async ({ page }) => {
    await waitForAppReady(page);
    const toggle = page.locator("[aria-label='Toggle proxy']").first();
    await toggle.click();
    await expect.poll(async () => (await getCallsFor(page, "ipc_toggle_proxy")).length).toBeGreaterThanOrEqual(1);
    const calls = await getCallsFor(page, "ipc_toggle_proxy");
    // First server proxy_running is false, so toggle should send enabled=true
    expect(calls[0].args.enabled).toBe(true);
  });
});

test.describe("Abnormal server pinning (U7)", () => {
  test("auth_failed server is pinned to top of list", async ({ page }) => {
    // Use custom servers with one in auth_failed state
    await mockTauri(page, {
      servers: [
        {
          id: "srv_normal", name: "Normal Server",
          ssh: { host: "1.1.1.1", port: 22, user: "root", auth_method: "password", key_path: "", key_auto_generated: false, connection_mode: "single", skip_hostkey_verify: false },
          proxy: { enabled: false, socks5_port: 1080, http_port: 8080, max_channels: 64, channel_idle_timeout: 300 },
          reconnect: { auto_reconnect: true, heartbeat_interval: 15, max_attempts: 10, initial_backoff_secs: 1, max_backoff_secs: 300 },
          ip_check: { enabled: false, interval_secs: 300 },
          last_known_ip: null, triggers: [], suppress_firewall_badge: false,
          current_status: "connected", current_ip: "1.1.1.1", connected_since: null,
          reconnect_count: 0, max_attempts: 10, proxy_running: false, active_channels: 0,
        },
        {
          id: "srv_abnormal", name: "Abnormal Server",
          ssh: { host: "2.2.2.2", port: 22, user: "root", auth_method: "password", key_path: "", key_auto_generated: false, connection_mode: "single", skip_hostkey_verify: false },
          proxy: { enabled: false, socks5_port: 1081, http_port: 8081, max_channels: 64, channel_idle_timeout: 300 },
          reconnect: { auto_reconnect: true, heartbeat_interval: 15, max_attempts: 10, initial_backoff_secs: 1, max_backoff_secs: 300 },
          ip_check: { enabled: false, interval_secs: 300 },
          last_known_ip: null, triggers: [], suppress_firewall_badge: false,
          current_status: "auth_failed", current_ip: null, connected_since: null,
          reconnect_count: 0, max_attempts: 10, proxy_running: false, active_channels: 0,
        },
      ],
    });
    await waitForAppReady(page, 5000);
    // Abnormal should appear before Normal in the list
    const items = page.locator("[role='listitem']");
    const firstItem = items.first();
    await expect(firstItem).toContainText("Abnormal Server", { timeout: 5000 });
  });
});

// === SECTION 2 END ===
