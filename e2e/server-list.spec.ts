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
    await expect(page.locator(".text-base.font-semibold:has-text('Tokyo VPS')")).toBeVisible({ timeout: 3000 });
  });

  test("selecting different server changes detail content", async ({ page }) => {
    await waitForAppReady(page);
    // Select first
    await page.locator("text=Tokyo VPS").first().click();
    await page.waitForTimeout(200);
    await expect(page.locator(".text-base.font-semibold:has-text('Tokyo VPS')")).toBeVisible({ timeout: 3000 });
    // Select second
    await page.locator("text=US West").first().click();
    await page.waitForTimeout(200);
    await expect(page.locator(".text-base.font-semibold:has-text('US West')")).toBeVisible({ timeout: 3000 });
  });
});

test.describe("Port copy chip (U8)", () => {
  test("port inputs show correct values in detail panel", async ({ page }) => {
    await waitForAppReady(page);
    await page.locator("text=Tokyo VPS").first().click();
    await page.waitForTimeout(300);
    // Port number inputs in detail panel: SOCKS5=1080, HTTP=8080
    const numInputs = page.locator("input[type='number']");
    await expect(numInputs.first()).toHaveValue("1080", { timeout: 5000 });
    await expect(numInputs.nth(1)).toHaveValue("8080", { timeout: 5000 });
  });

  test("port input value changes when switching servers", async ({ page }) => {
    await waitForAppReady(page);
    await page.locator("text=US West").first().click();
    await page.waitForTimeout(300);
    const numInputs = page.locator("input[type='number']");
    await expect(numInputs.first()).toHaveValue("1081", { timeout: 5000 });
  });
});

test.describe("Inline proxy toggle (U6)", () => {
  test("proxy toggle button exists in detail panel", async ({ page }) => {
    await waitForAppReady(page);
    await page.locator("text=Tokyo VPS").first().click();
    await page.waitForTimeout(300);
    // The Start Proxy button is in the detail panel
    await expect(page.locator("button:has-text('Start Proxy')")).toBeVisible({ timeout: 5000 });
  });

  test("clicking Start Proxy calls ipc_toggle_proxy", async ({ page }) => {
    await waitForAppReady(page);
    await page.locator("text=Tokyo VPS").first().click();
    await page.waitForTimeout(300);
    await page.locator("button:has-text('Start Proxy')").click();
    await expect.poll(async () => (await getCallsFor(page, "ipc_toggle_proxy")).length, { timeout: 5000 }).toBeGreaterThanOrEqual(1);
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
          reconnect: { auto_reconnect: true, heartbeat_interval: 15, max_attempts: 10, reconnect_timeout_secs: 86400, initial_backoff_secs: 1, max_backoff_secs: 300 },
          ip_check: { enabled: false, interval_secs: 300 },
          last_known_ip: null, triggers: [], suppress_firewall_badge: false,
          current_status: "connected", current_ip: "1.1.1.1", connected_since: null,
          reconnect_count: 0, max_attempts: 10, proxy_running: false, active_channels: 0,
        },
        {
          id: "srv_abnormal", name: "Abnormal Server",
          ssh: { host: "2.2.2.2", port: 22, user: "root", auth_method: "password", key_path: "", key_auto_generated: false, connection_mode: "single", skip_hostkey_verify: false },
          proxy: { enabled: false, socks5_port: 1081, http_port: 8081, max_channels: 64, channel_idle_timeout: 300 },
          reconnect: { auto_reconnect: true, heartbeat_interval: 15, max_attempts: 10, reconnect_timeout_secs: 86400, initial_backoff_secs: 1, max_backoff_secs: 300 },
          ip_check: { enabled: false, interval_secs: 300 },
          last_known_ip: null, triggers: [], suppress_firewall_badge: false,
          current_status: "auth_failed", current_ip: null, connected_since: null,
          reconnect_count: 0, max_attempts: 10, proxy_running: false, active_channels: 0,
        },
      ],
    });
    await waitForAppReady(page, 5000);
    // Abnormal should appear before Normal in the list (auth_failed is sorted to top)
    // Note: a "My Computer" local server may also appear; we check relative order
    const items = page.locator("[role='listitem']");
    const count = await items.count();
    let abnormalIdx = -1;
    let normalIdx = -1;
    for (let i = 0; i < count; i++) {
      const text = await items.nth(i).textContent() || "";
      if (text.includes("Abnormal Server")) abnormalIdx = i;
      if (text.includes("Normal Server")) normalIdx = i;
    }
    expect(abnormalIdx).toBeGreaterThanOrEqual(0);
    expect(normalIdx).toBeGreaterThanOrEqual(0);
    expect(abnormalIdx).toBeLessThan(normalIdx);
  });
});

// === SECTION 2 END ===
