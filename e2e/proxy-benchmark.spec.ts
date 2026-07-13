// E2E test: Proxy tab interactions (FP-9.4 / FP-9.9 / U6 / U18)
// Tests: proxy start/stop, port editing, set/clear system proxy,
// rapid server switching stability, proxy status display.

import { test, expect } from "@playwright/test";
import { mockTauri, waitForAppReady, getCallsFor, getMockStore } from "./fixtures";

// === SECTION 1 END ===

test.beforeEach(async ({ page }) => {
  await mockTauri(page);
});

test.describe("Proxy tab rendering", () => {
  test("proxy tab shows SOCKS5 and HTTP port inputs", async ({ page }) => {
    await waitForAppReady(page);
    await page.locator("text=Tokyo VPS").first().click();
    await page.waitForTimeout(300);
    await page.locator("button:has-text('Proxy')").first().click();
    await page.waitForTimeout(300);
    // Port number inputs should be visible with correct values
    // Order: mixed_port (0), socks5_port (1080), http_port (8080)
    const numInputs = page.locator("input[type='number']");
    const socksInput = numInputs.nth(1); // socks5_port is second
    await expect(socksInput).toHaveValue("1080", { timeout: 3000 });
    const httpInput = numInputs.nth(2); // http_port is third
    await expect(httpInput).toHaveValue("8080", { timeout: 3000 });
  });

  test("start proxy button is disabled when not connected", async ({ page }) => {
    await waitForAppReady(page);
    await page.locator("text=Tokyo VPS").first().click();
    await page.waitForTimeout(300);
    await page.locator("button:has-text('Proxy')").first().click();
    await page.waitForTimeout(300);
    const startBtn = page.locator("button:has-text('Start Proxy')");
    await expect(startBtn).toBeDisabled({ timeout: 3000 });
  });
});

// === SECTION 2 END ===

test.describe("Proxy start/stop (U6)", () => {
  test("start proxy calls ipc_toggle_proxy with enabled=true", async ({ page }) => {
    await waitForAppReady(page);
    await page.locator("text=Tokyo VPS").first().click();
    // Connect first — use detail panel's Connect button (not "Connect All")
    await page.locator("button.bg-blue-500:has-text('Connect')").click();
    await expect.poll(async () => (await getCallsFor(page, "ipc_connect_server")).length, { timeout: 5000 }).toBeGreaterThanOrEqual(1);
    await page.waitForTimeout(300);
    // Switch to Proxy tab
    await page.locator("button:has-text('Proxy')").first().click();
    await page.waitForTimeout(200);
    // Start proxy
    await page.locator("button:has-text('Start Proxy')").click();
    await expect.poll(async () => (await getCallsFor(page, "ipc_toggle_proxy")).length, { timeout: 5000 }).toBeGreaterThanOrEqual(1);
    const calls = await getCallsFor(page, "ipc_toggle_proxy");
    expect(calls[calls.length - 1].args.enabled).toBe(true);
  });

  test("stop proxy calls ipc_toggle_proxy with enabled=false", async ({ page }) => {
    await waitForAppReady(page);
    await page.locator("text=Tokyo VPS").first().click();
    // Connect + start proxy
    await page.locator("button.bg-blue-500:has-text('Connect')").click();
    await expect.poll(async () => (await getCallsFor(page, "ipc_connect_server")).length, { timeout: 5000 }).toBeGreaterThanOrEqual(1);
    await page.waitForTimeout(300);
    await page.locator("button:has-text('Proxy')").first().click();
    await page.waitForTimeout(200);
    await page.locator("button:has-text('Start Proxy')").click();
    await expect.poll(async () => (await getCallsFor(page, "ipc_toggle_proxy")).length, { timeout: 5000 }).toBeGreaterThanOrEqual(1);
    await page.waitForTimeout(300);
    // Now Stop Proxy button should be visible
    const stopBtn = page.locator("button:has-text('Stop Proxy')");
    await expect(stopBtn).toBeVisible({ timeout: 3000 });
    await stopBtn.click();
    await expect.poll(async () => (await getCallsFor(page, "ipc_toggle_proxy")).length, { timeout: 5000 }).toBeGreaterThanOrEqual(2);
    const calls = await getCallsFor(page, "ipc_toggle_proxy");
    expect(calls[calls.length - 1].args.enabled).toBe(false);
  });
});

test.describe("System proxy (U18)", () => {
  test("set as system proxy calls ipc_set_system_proxy", async ({ page }) => {
    await waitForAppReady(page);
    await page.locator("text=Tokyo VPS").first().click();
    // Connect + start proxy (system proxy requires proxy running)
    await page.locator("button.bg-blue-500:has-text('Connect')").click();
    await expect.poll(async () => (await getCallsFor(page, "ipc_connect_server")).length, { timeout: 5000 }).toBeGreaterThanOrEqual(1);
    await page.waitForTimeout(300);
    await page.locator("button:has-text('Proxy')").first().click();
    await page.waitForTimeout(200);
    await page.locator("button:has-text('Start Proxy')").click();
    await expect.poll(async () => (await getCallsFor(page, "ipc_toggle_proxy")).length, { timeout: 5000 }).toBeGreaterThanOrEqual(1);
    await page.waitForTimeout(300);
    // Click "Set as System Proxy"
    await page.locator("button:has-text('Set as System Proxy')").click();
    await expect.poll(async () => (await getCallsFor(page, "ipc_set_system_proxy")).length, { timeout: 5000 }).toBeGreaterThanOrEqual(1);
    const calls = await getCallsFor(page, "ipc_set_system_proxy");
    expect(calls[0].args.serverId).toBe("srv_1");
  });

  test("clear system proxy calls ipc_clear_system_proxy", async ({ page }) => {
    await waitForAppReady(page);
    await page.locator("text=Tokyo VPS").first().click();
    await page.locator("button:has-text('Proxy')").first().click();
    await page.waitForTimeout(200);
    // Use exact text match to avoid matching "Clear Logs"
    await page.locator("button", { hasText: /^Clear$/ }).click();
    await expect.poll(async () => (await getCallsFor(page, "ipc_clear_system_proxy")).length, { timeout: 5000 }).toBeGreaterThanOrEqual(1);
  });

  test("set as system proxy is disabled when proxy not running", async ({ page }) => {
    await waitForAppReady(page);
    await page.locator("text=Tokyo VPS").first().click();
    await page.locator("button:has-text('Proxy')").first().click();
    await page.waitForTimeout(200);
    const setBtn = page.locator("button:has-text('Set as System Proxy')");
    await expect(setBtn).toBeDisabled({ timeout: 3000 });
  });
});

test.describe("Port editing", () => {
  test("changing SOCKS5 port calls ipc_update_server", async ({ page }) => {
    await waitForAppReady(page);
    await page.locator("text=Tokyo VPS").first().click();
    await page.locator("button:has-text('Proxy')").first().click();
    await page.waitForTimeout(200);
    const socksInput = page.locator("input[type='number']").nth(1); // socks5_port is second
    await socksInput.fill("2080");
    await socksInput.press("Tab");
    await expect.poll(async () => (await getCallsFor(page, "ipc_update_server")).length, { timeout: 5000 }).toBeGreaterThanOrEqual(1);
    const calls = await getCallsFor(page, "ipc_update_server");
    expect(calls[0].args.socks5Port || calls[0].args.socks5_port).toBe(2080);
  });

  test("port inputs are disabled when proxy is running", async ({ page }) => {
    await waitForAppReady(page);
    await page.locator("text=Tokyo VPS").first().click();
    await page.locator("button.bg-blue-500:has-text('Connect')").click();
    await expect.poll(async () => (await getCallsFor(page, "ipc_connect_server")).length, { timeout: 5000 }).toBeGreaterThanOrEqual(1);
    await page.waitForTimeout(300);
    await page.locator("button:has-text('Proxy')").first().click();
    await page.waitForTimeout(200);
    await page.locator("button:has-text('Start Proxy')").click();
    await expect.poll(async () => (await getCallsFor(page, "ipc_toggle_proxy")).length, { timeout: 5000 }).toBeGreaterThanOrEqual(1);
    await page.waitForTimeout(300);
    // Port inputs should now be disabled (check socks5_port = nth(1))
    const socksInput = page.locator("input[type='number']").nth(1);
    await expect(socksInput).toBeDisabled({ timeout: 3000 });
  });
});

test.describe("Stability under rapid interaction (FP-9.9)", () => {
  test("rapid server switching does not crash the app", async ({ page }) => {
    await waitForAppReady(page);
    for (let i = 0; i < 10; i++) {
      await page.locator("text=Tokyo VPS").first().click();
      await page.waitForTimeout(50);
      await page.locator("text=US West").first().click();
      await page.waitForTimeout(50);
    }
    // App should still be responsive
    await expect(page.locator("body")).toBeVisible();
    await expect(page.locator("text=Tokyo VPS")).toBeVisible({ timeout: 3000 });
  });

  test("rapid connect/disconnect does not crash", async ({ page }) => {
    await waitForAppReady(page);
    await page.locator("text=Tokyo VPS").first().click();
    for (let i = 0; i < 5; i++) {
      // Use detail panel's Connect/Disconnect button (not "Connect All"/"Disconnect All")
      const connectBtn = page.locator("button.bg-blue-500:has-text('Connect')");
      if (await connectBtn.isVisible({ timeout: 1000 }).catch(() => false)) {
        await connectBtn.click();
        await page.waitForTimeout(200);
      }
      const disconnectBtn = page.locator("button.bg-red-500:has-text('Disconnect')");
      if (await disconnectBtn.isVisible({ timeout: 1000 }).catch(() => false)) {
        await disconnectBtn.click();
        await page.waitForTimeout(200);
      }
    }
    // App should still be responsive
    await expect(page.locator("body")).toBeVisible();
    const connectCalls = await getCallsFor(page, "ipc_connect_server");
    const disconnectCalls = await getCallsFor(page, "ipc_disconnect_server");
    expect(connectCalls.length).toBeGreaterThan(0);
    expect(disconnectCalls.length).toBeGreaterThan(0);
  });
});

// === SECTION 3 END ===
