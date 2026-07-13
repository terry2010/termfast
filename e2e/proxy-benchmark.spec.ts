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
    // Port number inputs: SOCKS5 is first, HTTP is second
    const numInputs = page.locator("input[type='number']");
    const socksInput = numInputs.first();
    await expect(socksInput).toHaveValue("1080", { timeout: 3000 });
    const httpInput = numInputs.nth(1);
    await expect(httpInput).toHaveValue("8080", { timeout: 3000 });
  });

  test("start proxy button is disabled when not connected", async ({ page }) => {
    await waitForAppReady(page);
    await page.locator("text=Tokyo VPS").first().click();
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
    await page.locator("button:has-text('Start Proxy')").click();
    await expect.poll(async () => (await getCallsFor(page, "ipc_toggle_proxy")).length, { timeout: 5000 }).toBeGreaterThanOrEqual(1);
    await page.waitForTimeout(300);
    // Click "Set as System Proxy" checkbox
    await page.locator("label:has-text('Set as System Proxy')").locator("input[type='checkbox']").check();
    await expect.poll(async () => (await getCallsFor(page, "ipc_set_system_proxy")).length, { timeout: 5000 }).toBeGreaterThanOrEqual(1);
    const calls = await getCallsFor(page, "ipc_set_system_proxy");
    expect(calls[0].args.serverId).toBe("srv_1");
  });

  test("clear system proxy calls ipc_clear_system_proxy", async ({ page }) => {
    await waitForAppReady(page);
    await page.locator("text=Tokyo VPS").first().click();
    // Connect + start proxy, then check and uncheck system proxy
    await page.locator("button.bg-blue-500:has-text('Connect')").click();
    await expect.poll(async () => (await getCallsFor(page, "ipc_connect_server")).length, { timeout: 5000 }).toBeGreaterThanOrEqual(1);
    await page.waitForTimeout(300);
    await page.locator("button:has-text('Start Proxy')").click();
    await expect.poll(async () => (await getCallsFor(page, "ipc_toggle_proxy")).length, { timeout: 5000 }).toBeGreaterThanOrEqual(1);
    await page.waitForTimeout(300);
    // Check then uncheck to trigger clear
    const sysProxyCheckbox = page.locator("label:has-text('Set as System Proxy')").locator("input[type='checkbox']");
    await sysProxyCheckbox.check();
    await expect.poll(async () => (await getCallsFor(page, "ipc_set_system_proxy")).length, { timeout: 5000 }).toBeGreaterThanOrEqual(1);
    await sysProxyCheckbox.uncheck();
    await expect.poll(async () => (await getCallsFor(page, "ipc_clear_system_proxy")).length, { timeout: 5000 }).toBeGreaterThanOrEqual(1);
  });

  test("set as system proxy is disabled when proxy not running", async ({ page }) => {
    await waitForAppReady(page);
    await page.locator("text=Tokyo VPS").first().click();
    const sysProxyCheckbox = page.locator("label:has-text('Set as System Proxy')").locator("input[type='checkbox']");
    await expect(sysProxyCheckbox).toBeDisabled({ timeout: 3000 });
  });
});

test.describe("Port editing", () => {
  test("changing SOCKS5 port calls ipc_update_server", async ({ page }) => {
    await waitForAppReady(page);
    await page.locator("text=Tokyo VPS").first().click();
    const socksInput = page.locator("input[type='number']").first(); // socks5_port is first
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
    await page.locator("button:has-text('Start Proxy')").click();
    await expect.poll(async () => (await getCallsFor(page, "ipc_toggle_proxy")).length, { timeout: 5000 }).toBeGreaterThanOrEqual(1);
    await page.waitForTimeout(300);
    // Port inputs should now be disabled (check socks5_port = first)
    const socksInput = page.locator("input[type='number']").first();
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
