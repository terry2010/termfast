// Unit tests for shouldResetOverlay — overlay dismissed-state reset logic
import { describe, it, expect } from "vitest";
import { shouldResetOverlay } from "../overlayReset";

describe("shouldResetOverlay", () => {
  // ── Case 1: status leaves "blocked" → always reset ─────────────────
  describe("status leaves blocked", () => {
    it("resets when status changes from blocked to working", () => {
      expect(shouldResetOverlay("blocked", "working", "Q1", "Q1")).toBe(true);
    });

    it("resets when status changes from blocked to idle", () => {
      expect(shouldResetOverlay("blocked", "idle", "Q1", "Q1")).toBe(true);
    });

    it("resets when status changes from blocked to done", () => {
      expect(shouldResetOverlay("blocked", "done", "Q1", "Q1")).toBe(true);
    });

    it("resets when status changes from blocked to unknown", () => {
      expect(shouldResetOverlay("blocked", "unknown", "Q1", "Q1")).toBe(true);
    });

    it("resets even if question also changed", () => {
      expect(shouldResetOverlay("blocked", "idle", "Q1", "Q2")).toBe(true);
    });

    it("resets even if question became null", () => {
      expect(shouldResetOverlay("blocked", "idle", "Q1", null)).toBe(true);
    });
  });

  // ── Case 2: still blocked but question changed → reset ──────────────
  describe("still blocked, question changed", () => {
    it("resets when question changes while blocked", () => {
      expect(shouldResetOverlay("blocked", "blocked", "Q1", "Q2")).toBe(true);
    });

    it("resets when question goes from null to non-null while blocked", () => {
      expect(shouldResetOverlay("blocked", "blocked", null, "Q1")).toBe(true);
    });

    it("does NOT reset when question is null (no new question)", () => {
      expect(shouldResetOverlay("blocked", "blocked", "Q1", null)).toBe(false);
    });

    it("does NOT reset when question unchanged while blocked", () => {
      expect(shouldResetOverlay("blocked", "blocked", "Q1", "Q1")).toBe(false);
    });

    it("does NOT reset when both questions are null while blocked", () => {
      expect(shouldResetOverlay("blocked", "blocked", null, null)).toBe(false);
    });
  });

  // ── Case 3: not blocked and no question change → no reset ───────────
  describe("no reset conditions", () => {
    it("does not reset when status is idle and unchanged", () => {
      expect(shouldResetOverlay("idle", "idle", null, null)).toBe(false);
    });

    it("does not reset when status is working and unchanged", () => {
      expect(shouldResetOverlay("working", "working", null, null)).toBe(false);
    });

    it("does not reset when status changes between non-blocked states", () => {
      expect(shouldResetOverlay("idle", "working", null, null)).toBe(false);
    });

    it("does not reset when entering blocked with same question", () => {
      expect(shouldResetOverlay("idle", "blocked", "Q1", "Q1")).toBe(false);
    });
  });

  // ── Edge cases ─────────────────────────────────────────────────────
  describe("edge cases", () => {
    it("resets when entering blocked with a new question (from null)", () => {
      // prevStatus=idle, currStatus=blocked, prevQuestion=null, currQuestion="Q1"
      // Case 1 doesn't apply (prevStatus !== "blocked")
      // Case 2: currStatus=blocked, currQuestion != null, currQuestion != prevQuestion
      expect(shouldResetOverlay("idle", "blocked", null, "Q1")).toBe(true);
    });

    it("handles empty string question as valid question", () => {
      // Empty string is not null, so it's a valid question
      expect(shouldResetOverlay("blocked", "blocked", "Q1", "")).toBe(true);
    });

    it("does not reset when question changes but status is not blocked", () => {
      expect(shouldResetOverlay("idle", "idle", "Q1", "Q2")).toBe(false);
    });
  });
});
