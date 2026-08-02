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

  test("start proxy button is visible when not connected", async ({ page }) => {
    await waitForAppReady(page);
    await page.locator("text=Tokyo VPS").first().click();
    await page.waitForTimeout(300);
    const startBtn = page.locator("button:has-text('Start Proxy')");
    await expect(startBtn).toBeVisible({ timeout: 3000 });
  });
});

// === SECTION 2 END ===

test.describe("Proxy start/stop (U6)", () => {
  test("start proxy calls ipc_toggle_proxy with enabled=true", async ({ page }) => {
    await waitForAppReady(page);
    await page.locator("text=Tokyo VPS").first().click();
    // Start Proxy button is visible in the overview (auto-connects if needed)
    await page.locator("button:has-text('Start Proxy')").click();
    await expect.poll(async () => (await getCallsFor(page, "ipc_toggle_proxy")).length, { timeout: 5000 }).toBeGreaterThanOrEqual(1);
    const calls = await getCallsFor(page, "ipc_toggle_proxy");
    expect(calls[calls.length - 1].args.enabled).toBe(true);
  });

  test("stop proxy calls ipc_toggle_proxy with enabled=false", async ({ page }) => {
    await waitForAppReady(page);
    await page.locator("text=Tokyo VPS").first().click();
    // Start proxy (auto-connects if needed)
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
    // Start proxy (auto-connects if needed)
    await page.locator("button:has-text('Start Proxy')").click();
    await expect.poll(async () => (await getCallsFor(page, "ipc_toggle_proxy")).length, { timeout: 5000 }).toBeGreaterThanOrEqual(1);
    await page.waitForTimeout(300);
    // Click "System Proxy" toggle (role=switch)
    await page.locator("text=System Proxy").locator("..").locator("button[role='switch']").click();
    await expect.poll(async () => (await getCallsFor(page, "ipc_set_system_proxy")).length, { timeout: 5000 }).toBeGreaterThanOrEqual(1);
    const calls = await getCallsFor(page, "ipc_set_system_proxy");
    expect(calls[0].args.serverId).toBe("srv_1");
  });

  test("clear system proxy calls ipc_clear_system_proxy", async ({ page }) => {
    await waitForAppReady(page);
    await page.locator("text=Tokyo VPS").first().click();
    // Start proxy (auto-connects if needed)
    await page.locator("button:has-text('Start Proxy')").click();
    await expect.poll(async () => (await getCallsFor(page, "ipc_toggle_proxy")).length, { timeout: 5000 }).toBeGreaterThanOrEqual(1);
    await page.waitForTimeout(300);
    // Check then uncheck to trigger clear
    const sysProxyToggle = page.locator("text=System Proxy").locator("..").locator("button[role='switch']");
    await sysProxyToggle.click();
    await expect.poll(async () => (await getCallsFor(page, "ipc_set_system_proxy")).length, { timeout: 5000 }).toBeGreaterThanOrEqual(1);
    await sysProxyToggle.click();
    await expect.poll(async () => (await getCallsFor(page, "ipc_clear_system_proxy")).length, { timeout: 5000 }).toBeGreaterThanOrEqual(1);
  });

  test("set as system proxy is disabled when proxy not running", async ({ page }) => {
    await waitForAppReady(page);
    await page.locator("text=Tokyo VPS").first().click();
    // System Proxy toggle is wrapped in a div with opacity-50 pointer-events-none
    // when proxy is not running. The toggle's parent div intercepts pointer events,
    // so clicking it should not trigger IPC. Verify no IPC call is made.
    const sysProxyToggle = page.locator("text=System Proxy").locator("..").locator("button[role='switch']");
    // The toggle exists but is non-interactive (pointer-events-none on parent)
    await expect(sysProxyToggle).toBeVisible({ timeout: 3000 });
    // Try clicking — should be a no-op due to pointer-events-none
    await sysProxyToggle.click({ timeout: 3000 }).catch(() => {});
    await page.waitForTimeout(500);
    const calls = await getCallsFor(page, "ipc_set_system_proxy");
    expect(calls.length).toBe(0);
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
    // Start proxy (auto-connects if needed)
    await page.locator("button:has-text('Start Proxy')").click();
    await expect.poll(async () => (await getCallsFor(page, "ipc_toggle_proxy")).length, { timeout: 5000 }).toBeGreaterThanOrEqual(1);
    await page.waitForTimeout(300);
    // When proxy is running, port inputs are replaced by static spans (not editable)
    // So the number input should no longer exist
    const socksInput = page.locator("input[type='number']").first();
    await expect(socksInput).toHaveCount(0, { timeout: 3000 });
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
      const connectBtn = page.locator("button:has-text('Connect Terminal')");
      if (await connectBtn.isVisible({ timeout: 1000 }).catch(() => false)) {
        await connectBtn.click();
        await page.waitForTimeout(500);
        // Click Overview tab to return to overview
        const overviewTab = page.locator("text=Overview").first();
        if (await overviewTab.isVisible({ timeout: 1000 }).catch(() => false)) {
          await overviewTab.click();
          await page.waitForTimeout(200);
        }
      }
      const disconnectBtn = page.locator("button:has-text('Disconnect Server')");
      if (await disconnectBtn.isVisible({ timeout: 1000 }).catch(() => false)) {
        await disconnectBtn.click();
        // If a confirmation dialog appears (due to active terminals), confirm it
        const dialog = page.locator(".fixed.inset-0").last();
        if (await dialog.isVisible({ timeout: 500 }).catch(() => false)) {
          await dialog.locator("button:has-text('Disconnect')").click();
        }
        await page.waitForTimeout(200);
      }
    }
    // App should still be responsive
    await expect(page.locator("body")).toBeVisible();
    const connectCalls = await getCallsFor(page, "ipc_connect_server");
    expect(connectCalls.length).toBeGreaterThan(0);
  });
});

// === SECTION 3 END ===
