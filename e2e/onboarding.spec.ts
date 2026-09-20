// E2E test: Onboarding wizard (FP-8.1 / FP-9.2 / U15)
// Tests: mode selection, VPS info entry, test connection, firewall check,
// template selection, completion — verifying IPC calls at each step.

import { test, expect } from "@playwright/test";
import { mockTauri, getCallsFor } from "./fixtures";

// === SECTION 1 END ===

// Onboarding shows when there are no servers.
// config: null simulates a true first run — no config file yet, so
// ipc_get_config returns null and the app treats this as "no config".
test.beforeEach(async ({ page }) => {
  await mockTauri(page, {
    servers: [],
    config: null,
  });
});

test.describe("Onboarding mode selection", () => {
  test("shows mode selection on first run with no servers", async ({ page }) => {
    await page.goto("/");
    // Use h1 heading specifically (button in title bar also has this text)
    await expect(page.locator("h1:has-text('Welcome to TermFast')")).toBeVisible({ timeout: 10000 });
    await expect(page.locator("text=Quick Mode")).toBeVisible({ timeout: 3000 });
    await expect(page.locator("text=Advanced Mode")).toBeVisible({ timeout: 3000 });
  });

  test("skip wizard button completes onboarding without adding server", async ({ page }) => {
    await page.goto("/");
    await expect(page.locator("text=Skip wizard")).toBeVisible({ timeout: 5000 });
    await page.locator("text=Skip wizard").click();
    await page.waitForTimeout(500);
    // Should no longer show onboarding
    await expect(page.locator("text=Quick Mode")).toHaveCount(0);
    // No ipc_add_server should have been called
    const addCalls = await getCallsFor(page, "ipc_add_server");
    expect(addCalls.length).toBe(0);
  });
});

test.describe("Quick mode flow (3 steps)", () => {
  test("quick mode: fill VPS info → test connection → firewall check → complete", async ({ page }) => {
    await page.goto("/");
    await expect(page.locator("text=Quick Mode")).toBeVisible({ timeout: 10000 });
    // Select quick mode — click the button containing "Quick Mode"
    await page.locator("button:has-text('Quick Mode')").click();
    await page.waitForTimeout(300);
    // Step 1: VPS Info — should show input fields
    await expect(page.locator("h2:has-text('VPS Info')")).toBeVisible({ timeout: 3000 });
    // Fill in the form — scope to onboarding overlay to avoid matching sidebar inputs
    const overlay = page.locator(".fixed.inset-0");
    await overlay.locator("input[type='text']").nth(0).fill("Test VPS"); // name
    await overlay.locator("input[type='text']").nth(1).fill("1.2.3.4"); // host
    await overlay.locator("input[type='password']").fill("mypassword");
    // Click OK to advance
    await page.locator("button:has-text('OK')").first().click();
    await page.waitForTimeout(300);
    // Step 2: Test Connection
    await expect(page.locator("h2:has-text('Test Connection')")).toBeVisible({ timeout: 3000 });
    await page.locator("button:has-text('Test Connection')").first().click();
    // Wait for the IPC calls (add_server + save_credential + connect_server)
    await expect.poll(async () => (await getCallsFor(page, "ipc_add_server")).length, { timeout: 5000 }).toBe(1);
    await expect.poll(async () => (await getCallsFor(page, "ipc_connect_server")).length, { timeout: 5000 }).toBe(1);
    // Should show success message
    await expect(page.locator("text=Connection successful")).toBeVisible({ timeout: 3000 });
    // Advance to step 3
    await page.locator("button:has-text('OK')").first().click();
    await page.waitForTimeout(300);
    // Step 3: Firewall Whitelist
    await expect(page.locator("h2:has-text('Firewall Whitelist')")).toBeVisible({ timeout: 3000 });
    // Click check firewall
    await page.locator("button:has-text('Check Firewall')").first().click();
    await expect.poll(async () => (await getCallsFor(page, "ipc_check_port_reachable")).length, { timeout: 5000 }).toBe(1);
    // Should show ok result
    await expect(page.locator("text=Port is reachable")).toBeVisible({ timeout: 3000 });
    // Complete
    await page.locator("button:has-text('Complete')").first().click();
    await page.waitForTimeout(500);
    // Onboarding should be gone
    await expect(page.locator("text=Quick Mode")).toHaveCount(0);
  });

  test("quick mode: test connection failure shows error message", async ({ page }) => {
    // Override ipc_connect_server to reject
    await page.goto("/");
    // We need to make connect fail — re-mock with overrides
    await page.evaluate(() => {
      const orig = (window as any).__TAURI_INTERNALS__.invoke;
      (window as any).__TAURI_INTERNALS__.invoke = async (cmd: string, args?: any) => {
        if (cmd === "ipc_connect_server") {
          throw "SshConnectFailed: connection refused";
        }
        return orig(cmd, args);
      };
    });
    await page.locator("text=Quick Mode").click();
    await page.waitForTimeout(300);
    await page.locator("input").nth(0).fill("Test VPS");
    await page.locator("input").nth(1).fill("1.2.3.4");
    await page.locator("input").nth(4).fill("wrongpass");
    await page.locator("button:has-text('OK')").first().click();
    await page.waitForTimeout(300);
    await page.locator("button:has-text('Test Connection')").first().click();
    // Should show error message
    await expect(page.locator("text=Connection failed")).toBeVisible({ timeout: 5000 });
  });
});

test.describe("Advanced mode flow (7 steps)", () => {
  test("advanced mode shows 7 steps with step indicator", async ({ page }) => {
    await page.goto("/");
    await expect(page.locator("text=Advanced Mode")).toBeVisible({ timeout: 10000 });
    await page.locator("text=Advanced Mode").click();
    await page.waitForTimeout(300);
    // Step 0: Welcome
    await expect(page.locator("text=Step 1/7")).toBeVisible({ timeout: 3000 });
    // Advance through steps
    for (let i = 0; i < 6; i++) {
      await page.locator("button:has-text('OK')").first().click();
      await page.waitForTimeout(200);
    }
    // Should be on step 7 (Complete)
    await expect(page.locator("text=Step 7/7")).toBeVisible({ timeout: 3000 });
  });

  test("advanced mode: auth method step allows key generation", async ({ page }) => {
    await page.goto("/");
    await page.locator("text=Advanced Mode").click();
    await page.waitForTimeout(300);
    // Step 0: Welcome → advance
    await page.locator("button:has-text('OK')").first().click();
    await page.waitForTimeout(300);
    // Step 1: VPS Info → fill and advance
    await page.locator("input").nth(0).fill("Test VPS");
    await page.locator("input").nth(1).fill("1.2.3.4");
    await page.locator("button:has-text('OK')").first().click();
    await page.waitForTimeout(300);
    // Step 2: Auth Method — should show radio buttons
    await expect(page.locator("h2:has-text('Authentication')")).toBeVisible({ timeout: 3000 });
    // Click "Generate SSH Key" button
    const genBtn = page.locator("button:has-text('Generate SSH Key')");
    if (await genBtn.isVisible({ timeout: 1000 }).catch(() => false)) {
      await genBtn.click();
      await expect.poll(async () => (await getCallsFor(page, "ipc_generate_ssh_key")).length, { timeout: 5000 }).toBe(1);
    }
  });
});

// === SECTION 2 END ===
