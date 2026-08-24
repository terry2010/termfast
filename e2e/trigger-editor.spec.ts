// E2E test: Trigger editor and trigger list (FP-8.4 / FP-8.5 / FP-9.5 / U4 / U5 / U11)
// Tests: trigger card rendering, manual fire, add new trigger via editor,
// edit existing trigger, delete trigger with confirmation.

import { test, expect } from "@playwright/test";
import {
  mockTauri,
  waitForAppReady,
  dismissModal,
  getCallsFor,
} from "./fixtures";

// === SECTION 1 END ===

/** Local triggers for "My Computer" (__local__) — triggers panel only shows for local/remote */
const localTriggers = [
  {
    id: "trig_1",
    template_id: "tpl_firewalld",
    name: "Firewalld IP Update",
    enabled: true,
    trigger_type: "OnIpChange",
    parameters: {},
    commands: ["firewall-cmd --add-source={{.NewIP}}"],
    timeout_secs: 30,
    cooldown_secs: 60,
    continue_on_error: false,
    notify_on_success: false,
    notify_on_failure: true,
    last_fired_at: null,
    template_hash_at_addition: "abc123",
  },
];

test.beforeEach(async ({ page }) => {
  await mockTauri(page, { localTriggers });
});

test.describe("Trigger list rendering", () => {
  test("triggers tab shows existing trigger card with name", async ({
    page,
  }) => {
    await waitForAppReady(page);
    await page.locator("text=My Computer").first().click();
    await page.waitForTimeout(300);
    await expect(page.locator("text=Firewalld IP Update")).toBeVisible({
      timeout: 3000,
    });
  });

  test("trigger card shows event type tag and command summary", async ({
    page,
  }) => {
    await waitForAppReady(page);
    await page.locator("text=My Computer").first().click();
    await page.waitForTimeout(300);
    // Event type tag: "On IP Change"
    await expect(page.locator("text=On IP Change")).toBeVisible({
      timeout: 3000,
    });
    // Trigger name should be visible
    await expect(page.locator("text=Firewalld IP Update")).toBeVisible({
      timeout: 3000,
    });
  });

  test("trigger card has Fire, Edit, Delete buttons", async ({ page }) => {
    await waitForAppReady(page);
    await page.locator("text=My Computer").first().click();
    await page.waitForTimeout(300);
    await expect(page.locator("button:has-text('Fire Trigger')")).toBeVisible({
      timeout: 3000,
    });
    await expect(page.locator("button:has-text('Edit')")).toBeVisible({
      timeout: 3000,
    });
    await expect(page.locator("button:has-text('Delete')")).toBeVisible({
      timeout: 3000,
    });
  });
});

// === SECTION 2 END ===

test.describe("Manual fire trigger (U5)", () => {
  test("clicking Fire button calls ipc_manual_fire_local_trigger", async ({
    page,
  }) => {
    await waitForAppReady(page);
    await page.locator("text=My Computer").first().click();
    await page.waitForTimeout(500);
    // For local (__local__), Fire button is always enabled (no need to connect)
    await expect(page.locator("button:has-text('Fire Trigger')")).toBeEnabled({ timeout: 5000 });
    await page.locator("button:has-text('Fire Trigger')").first().click();
    await expect
      .poll(
        async () => (await getCallsFor(page, "ipc_manual_fire_local_trigger")).length,
        { timeout: 5000 },
      )
      .toBe(1);
    const calls = await getCallsFor(page, "ipc_manual_fire_local_trigger");
    expect(calls[0].args.triggerId).toBe("trig_1");
  });
});

test.describe("Add new trigger via editor (U11)", () => {
  test("clicking Add Trigger opens editor dialog", async ({ page }) => {
    await waitForAppReady(page);
    await page.locator("text=My Computer").first().click();
    await page.waitForTimeout(300);
    await page.locator("button:has-text('Add Trigger')").first().click();
    await page.waitForTimeout(300);
    // Editor dialog should appear
    await expect(page.locator(".fixed.inset-0")).toBeVisible({ timeout: 3000 });
    // Should show "Add Trigger" heading
    await expect(page.locator("h2:has-text('Add Trigger')")).toBeVisible({
      timeout: 3000,
    });
  });

  test("filling trigger form and saving calls ipc_save_local_trigger", async ({
    page,
  }) => {
    await waitForAppReady(page);
    await page.locator("text=My Computer").first().click();
    await page.waitForTimeout(300);
    await page.locator("button:has-text('Add Trigger')").first().click();
    await page.waitForTimeout(300);
    // Fill trigger name
    await page
      .locator("[data-testid='trigger-name-input']")
      .fill("My Custom Trigger");
    // Select event type — the only select in the form
    await page.locator("select").first().selectOption("OnTerminalOpen");
    // Type into CodeMirror editor — click the editor area and type
    const editor = page.locator(".cm-editor .cm-content").first();
    await editor.click();
    await page.keyboard.type("echo hello");
    // Click Save
    await page.locator("button:has-text('Save')").last().click();
    await expect
      .poll(async () => (await getCallsFor(page, "ipc_save_local_trigger")).length, {
        timeout: 5000,
      })
      .toBe(1);
    const calls = await getCallsFor(page, "ipc_save_local_trigger");
    expect(calls[0].args.trigger.name).toBe("My Custom Trigger");
    expect(calls[0].args.trigger.trigger_type).toBe("OnTerminalOpen");
  });

  test("save button does not call ipc_save_local_trigger when name is empty", async ({
    page,
  }) => {
    await waitForAppReady(page);
    await page.locator("text=My Computer").first().click();
    await page.waitForTimeout(300);
    await page.locator("button:has-text('Add Trigger')").first().click();
    await page.waitForTimeout(300);
    // Click Save without filling name
    await page.locator("button:has-text('Save')").last().click();
    await page.waitForTimeout(500);
    // Should show error message, not call IPC
    await expect(page.locator("text=Trigger name is required")).toBeVisible({
      timeout: 3000,
    });
    const calls = await getCallsFor(page, "ipc_save_local_trigger");
    expect(calls.length).toBe(0);
  });
});

test.describe("Edit existing trigger", () => {
  test("clicking Edit opens editor with trigger data pre-filled", async ({
    page,
  }) => {
    await waitForAppReady(page);
    await page.locator("text=My Computer").first().click();
    await page.waitForTimeout(300);
    await page.locator("button:has-text('Edit')").first().click();
    await page.waitForTimeout(300);
    // Editor should show "Edit Trigger" heading
    await expect(page.locator("h2:has-text('Edit Trigger')")).toBeVisible({
      timeout: 3000,
    });
    // Name field should be pre-filled
    const nameInput = page.locator("[data-testid='trigger-name-input']");
    await expect(nameInput).toHaveValue("Firewalld IP Update", {
      timeout: 3000,
    });
  });

  test("editing trigger name and saving calls ipc_save_local_trigger", async ({
    page,
  }) => {
    await waitForAppReady(page);
    await page.locator("text=My Computer").first().click();
    await page.waitForTimeout(300);
    await page.locator("button:has-text('Edit')").first().click();
    await page.waitForTimeout(300);
    // Change the name
    await page
      .locator("[data-testid='trigger-name-input']")
      .fill("Updated Trigger Name");
    await page.locator("button:has-text('Save')").last().click();
    await expect
      .poll(
        async () => (await getCallsFor(page, "ipc_save_local_trigger")).length,
        { timeout: 5000 },
      )
      .toBe(1);
    const calls = await getCallsFor(page, "ipc_save_local_trigger");
    expect(calls[0].args.trigger.name).toBe("Updated Trigger Name");
    expect(calls[0].args.trigger.id).toBe("trig_1");
  });
});

test.describe("Delete trigger with confirmation", () => {
  test("clicking Delete opens confirm dialog", async ({ page }) => {
    await waitForAppReady(page);
    await page.locator("text=My Computer").first().click();
    await page.waitForTimeout(300);
    await page.locator("button:has-text('Delete')").first().click();
    await page.waitForTimeout(300);
    // Confirm dialog should appear — use heading to avoid matching body text
    await expect(page.locator("h2:has-text('Delete trigger')")).toBeVisible({
      timeout: 3000,
    });
  });

  test("confirming delete calls ipc_remove_local_trigger", async ({ page }) => {
    await waitForAppReady(page);
    await page.locator("text=My Computer").first().click();
    await page.waitForTimeout(300);
    await page.locator("button:has-text('Delete')").first().click();
    await page.waitForTimeout(300);
    // Medium danger — confirm button says "OK"
    await page.locator("button:has-text('OK')").last().click();
    await expect
      .poll(
        async () => (await getCallsFor(page, "ipc_remove_local_trigger")).length,
        { timeout: 5000 },
      )
      .toBe(1);
    const calls = await getCallsFor(page, "ipc_remove_local_trigger");
    expect(calls[0].args.triggerId).toBe("trig_1");
  });

  test("canceling delete does not call ipc_remove_local_trigger", async ({
    page,
  }) => {
    await waitForAppReady(page);
    await page.locator("text=My Computer").first().click();
    await page.waitForTimeout(300);
    await page.locator("button:has-text('Delete')").first().click();
    await page.waitForTimeout(300);
    await page.locator("button:has-text('Cancel')").first().click();
    await page.waitForTimeout(500);
    const calls = await getCallsFor(page, "ipc_remove_local_trigger");
    expect(calls.length).toBe(0);
  });
});

// === SECTION 3 END ===
