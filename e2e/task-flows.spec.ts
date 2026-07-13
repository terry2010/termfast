// E2E task flow tests — FP-9.1 to FP-9.8
// Real UI interactions: click buttons, fill forms, verify state changes
// and verify that the correct IPC calls were made to the backend.

import { test, expect } from "@playwright/test";
import { mockTauri, waitForAppReady, getCallsFor, getMockStore, defaultServers } from "./fixtures";

// === SECTION 1 END ===

test.beforeEach(async ({ page }) => {
  await mockTauri(page);
});

// FP-9.1 / U3: Add server — fill form, submit, verify IPC + list update
test.describe("FP-9.1: Add Server", () => {
  test("fill add-server form and submit calls ipc_add_server", async ({ page }) => {
    await waitForAppReady(page);
    // Click the "+ Add Server" button in the sidebar
    await page.locator("button:has-text('Add Server')").first().click();
    // Dialog should appear
    await expect(page.locator(".fixed.inset-0.bg-black\\/50")).toBeVisible({ timeout: 3000 });
    // Fill the form — use dialog-scoped selectors (text inputs exclude number/password)
    const dialog = page.locator(".fixed.inset-0.bg-black\\/50");
    const textInputs = dialog.locator("input:not([type='number']):not([type='password'])");
    await textInputs.nth(0).fill("My New VPS"); // name
    await textInputs.nth(1).fill("9.8.7.6"); // host
    await textInputs.nth(2).fill("myuser"); // username
    // Submit
    await page.locator("button:has-text('Add')").last().click();
    // Dialog should close
    await expect(page.locator(".fixed.inset-0.bg-black\\/50")).toHaveCount(0, { timeout: 3000 });
    // Verify ipc_add_server was called with correct host
    const addCalls = await getCallsFor(page, "ipc_add_server");
    expect(addCalls.length).toBe(1);
    expect(addCalls[0].args.config.ssh.host).toBe("9.8.7.6");
    expect(addCalls[0].args.config.name).toBe("My New VPS");
  });

  test("add button disabled when name or host is empty", async ({ page }) => {
    await waitForAppReady(page);
    await page.locator("button:has-text('Add Server')").first().click();
    await expect(page.locator(".fixed.inset-0.bg-black\\/50")).toBeVisible({ timeout: 3000 });
    // Add button should be disabled initially
    const dialog = page.locator(".fixed.inset-0.bg-black\\/50");
    const addBtn = dialog.locator("button:has-text('Add')").last();
    const textInputs = dialog.locator("input:not([type='number']):not([type='password'])");
    await expect(addBtn).toBeDisabled();
    // Fill name only — still disabled (host missing)
    await textInputs.nth(0).fill("Test");
    await expect(addBtn).toBeDisabled();
    // Fill host — now enabled
    await textInputs.nth(1).fill("1.2.3.4");
    await expect(addBtn).toBeEnabled();
  });

  test("cancel button closes dialog without calling ipc_add_server", async ({ page }) => {
    await waitForAppReady(page);
    await page.locator("button:has-text('Add Server')").first().click();
    await expect(page.locator(".fixed.inset-0.bg-black\\/50")).toBeVisible({ timeout: 3000 });
    await page.locator("button:has-text('Cancel')").first().click();
    await expect(page.locator(".fixed.inset-0.bg-black\\/50")).toHaveCount(0, { timeout: 3000 });
    const addCalls = await getCallsFor(page, "ipc_add_server");
    expect(addCalls.length).toBe(0);
  });
});

// FP-9.1 / U2: Connect server — click connect, verify status change + IPC
test.describe("FP-9.1: Connect & Disconnect", () => {
  test("click connect calls ipc_connect_server and shows connected status", async ({ page }) => {
    await waitForAppReady(page);
    // Click on first server to select it
    await page.locator("text=Tokyo VPS").first().click();
    await page.waitForTimeout(300);
    // Detail panel should show Connect button (not "Connect All" which is in sidebar)
    const connectBtn = page.locator("h2:has-text('Tokyo VPS') + div button:has-text('Connect'), button.bg-blue-500:has-text('Connect')");
    await expect(connectBtn).toBeVisible({ timeout: 3000 });
    await connectBtn.click();
    // Wait for IPC call — use toBeGreaterThanOrEqual since app may auto-reconnect
    await expect.poll(async () => (await getCallsFor(page, "ipc_connect_server")).length, { timeout: 5000 }).toBeGreaterThanOrEqual(1);
    // The mock store should now show connected status
    const store = await getMockStore(page);
    const srv = store.servers.find((s) => s.id === "srv_1");
    expect(srv?.current_status).toBe("connected");
  });

  test("click disconnect calls ipc_disconnect_server", async ({ page }) => {
    await waitForAppReady(page);
    await page.locator("text=Tokyo VPS").first().click();
    // Connect first — use the detail panel's Connect button (not "Connect All")
    const connectBtn = page.locator("button.bg-blue-500:has-text('Connect')");
    await connectBtn.click();
    await expect.poll(async () => (await getCallsFor(page, "ipc_connect_server")).length, { timeout: 5000 }).toBeGreaterThanOrEqual(1);
    await page.waitForTimeout(300);
    // Now disconnect button should be visible in detail panel
    const disconnectBtn = page.locator("button.bg-red-500:has-text('Disconnect')");
    await expect(disconnectBtn).toBeVisible({ timeout: 3000 });
    await disconnectBtn.click();
    await expect.poll(async () => (await getCallsFor(page, "ipc_disconnect_server")).length, { timeout: 5000 }).toBeGreaterThanOrEqual(1);
  });
});

// FP-9.2 / U7: 5-state visual indicator — verify status dot renders
test.describe("FP-9.2: Server Status Display", () => {
  test("server detail shows host:port and status text", async ({ page }) => {
    await waitForAppReady(page);
    await page.locator("text=Tokyo VPS").first().click();
    await page.waitForTimeout(300);
    // Connection tab should show host:port
    await expect(page.locator("text=1.2.3.4:22")).toBeVisible({ timeout: 3000 });
    // Status label should be visible
    await expect(page.locator("text=Disconnected")).toBeVisible({ timeout: 3000 });
  });
});

// FP-9.3 / U4: Trigger list — switch to triggers tab, verify content
test.describe("FP-9.3: Trigger Tab", () => {
  test("triggers tab shows empty state when no triggers", async ({ page }) => {
    await waitForAppReady(page);
    await page.locator("text=Tokyo VPS").first().click();
    await page.waitForTimeout(300);
    // Click Triggers tab
    await page.locator("button:has-text('Triggers')").first().click();
    await page.waitForTimeout(200);
    // Should show the "Add Trigger" button
    await expect(page.locator("button:has-text('Add Trigger')")).toBeVisible({ timeout: 3000 });
  });
});

// FP-9.4 / U6: Proxy toggle — switch to proxy tab, start proxy, verify
test.describe("FP-9.4: Proxy Toggle", () => {
  test("start proxy button calls ipc_toggle_proxy with enabled=true", async ({ page }) => {
    await waitForAppReady(page);
    await page.locator("text=Tokyo VPS").first().click();
    // Connect first (proxy start requires connected state) — use detail panel's button
    await page.locator("button.bg-blue-500:has-text('Connect')").click();
    await expect.poll(async () => (await getCallsFor(page, "ipc_connect_server")).length, { timeout: 5000 }).toBeGreaterThanOrEqual(1);
    await page.waitForTimeout(300);
    // Switch to Proxy tab
    await page.locator("button:has-text('Proxy')").first().click();
    await page.waitForTimeout(200);
    // Start Proxy button should be visible
    const startBtn = page.locator("button:has-text('Start Proxy')");
    await expect(startBtn).toBeVisible({ timeout: 3000 });
    await startBtn.click();
    // Verify ipc_toggle_proxy was called with enabled=true
    const toggleCalls = await getCallsFor(page, "ipc_toggle_proxy");
    expect(toggleCalls.length).toBeGreaterThanOrEqual(1);
    expect(toggleCalls[toggleCalls.length - 1].args.enabled).toBe(true);
  });

  test("proxy tab shows SOCKS5 and HTTP port numbers", async ({ page }) => {
    await waitForAppReady(page);
    await page.locator("text=Tokyo VPS").first().click();
    await page.locator("button:has-text('Proxy')").first().click();
    await page.waitForTimeout(200);
    // Should show port text in the proxy tab (SOCKS5 :1080  HTTP :8080)
    await expect(page.locator("text=/SOCKS5 :1080/")).toBeVisible({ timeout: 3000 });
  });
});

// FP-9.5 / U13: Settings page — open, verify sections, change theme
test.describe("FP-9.5: Settings", () => {
  test("settings dialog opens with all sections", async ({ page }) => {
    await waitForAppReady(page);
    // Click Settings button in title bar
    await page.locator("button:has-text('Settings')").first().click();
    await expect(page.locator(".fixed.inset-0.bg-black\\/50")).toBeVisible({ timeout: 3000 });
    // Verify section headings are present
    await expect(page.locator("h3:has-text('General')")).toBeVisible({ timeout: 3000 });
    await expect(page.locator("h3:has-text('Logs')")).toBeVisible();
    await expect(page.locator("h3:has-text('Notifications')")).toBeVisible();
    await expect(page.locator("h3:has-text('About')")).toBeVisible();
  });

  test("changing theme calls ipc_update_general_config", async ({ page }) => {
    await waitForAppReady(page);
    await page.locator("button:has-text('Settings')").first().click();
    await expect(page.locator(".fixed.inset-0.bg-black\\/50")).toBeVisible({ timeout: 3000 });
    // Find the theme select (second select in General section)
    const dialog = page.locator(".fixed.inset-0.bg-black\\/50");
    const themeSelect = dialog.locator("select").nth(1);
    await themeSelect.selectOption("dark");
    await expect.poll(async () => (await getCallsFor(page, "ipc_update_general_config")).length).toBeGreaterThanOrEqual(1);
    const cfgCalls = await getCallsFor(page, "ipc_update_general_config");
    const lastCall = cfgCalls[cfgCalls.length - 1];
    expect(lastCall.args.theme).toBe("dark");
  });

  test("close button closes settings dialog", async ({ page }) => {
    await waitForAppReady(page);
    await page.locator("button:has-text('Settings')").first().click();
    await expect(page.locator(".fixed.inset-0.bg-black\\/50")).toBeVisible({ timeout: 3000 });
    // Click the ✕ close button inside the dialog
    await page.locator("[aria-label='Close']").first().click();
    await expect(page.locator(".fixed.inset-0.bg-black\\/50")).toHaveCount(0, { timeout: 3000 });
  });
});

// FP-9.6 / U10: Log panel — expand, verify filters
test.describe("FP-9.6: Log Panel", () => {
  test("clicking log panel expands it with filters", async ({ page }) => {
    await waitForAppReady(page);
    // Log panel collapsed button at bottom — click the title button (first button in border-t)
    const logBtn = page.locator(".border-t button").first();
    await expect(logBtn).toBeVisible({ timeout: 5000 });
    await logBtn.click();
    await page.waitForTimeout(500);
    // Expanded panel should have level filter (select), search input
    await expect(page.locator("select").filter({ hasText: /all|info|warn|error/i }).first()).toBeVisible({ timeout: 5000 });
  });
});

// FP-9.7 / U20: Template library — open, verify templates
test.describe("FP-9.7: Template Library", () => {
  test("template library opens and shows built-in templates", async ({ page }) => {
    await waitForAppReady(page);
    // Click "Template Library" button in sidebar
    await page.locator("button:has-text('Template Library')").first().click();
    await expect(page.locator(".fixed.inset-0.bg-black\\/50")).toBeVisible({ timeout: 3000 });
    // Should show template names from mock config
    await expect(page.locator("text=Firewalld IP Update")).toBeVisible({ timeout: 5000 });
    await expect(page.locator("text=UFW IP Update")).toBeVisible({ timeout: 3000 });
  });

  test("clicking a template expands its commands", async ({ page }) => {
    await waitForAppReady(page);
    await page.locator("button:has-text('Template Library')").first().click();
    await expect(page.locator(".fixed.inset-0.bg-black\\/50")).toBeVisible({ timeout: 3000 });
    // Click on the Firewalld template
    await page.locator("text=Firewalld IP Update").first().click();
    await page.waitForTimeout(200);
    // Expanded view should show the command in a <pre> block
    await expect(page.locator("pre").filter({ hasText: "firewall-cmd" })).toBeVisible({ timeout: 3000 });
  });
});

// FP-9.8 / U14: Multi-server — switch between servers, verify detail updates
test.describe("FP-9.8: Multi-Server", () => {
  test("switching servers updates detail panel header", async ({ page }) => {
    await waitForAppReady(page);
    // Click first server
    await page.locator("text=Tokyo VPS").first().click();
    await page.waitForTimeout(300);
    await expect(page.locator("h2:has-text('Tokyo VPS')")).toBeVisible({ timeout: 3000 });
    // Click second server
    await page.locator("text=US West").first().click();
    await page.waitForTimeout(300);
    await expect(page.locator("h2:has-text('US West')")).toBeVisible({ timeout: 3000 });
    // First server header should no longer be the active detail
    await expect(page.locator("h2:has-text('Tokyo VPS')")).toHaveCount(0);
  });

  test("both servers show their respective port chips", async ({ page }) => {
    await waitForAppReady(page);
    await expect(page.locator("text=:1080")).toBeVisible({ timeout: 5000 });
    await expect(page.locator("text=:1081")).toBeVisible({ timeout: 5000 });
  });
});

// === SECTION 2 END ===
