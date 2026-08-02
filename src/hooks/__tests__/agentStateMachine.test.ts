// Unit tests for agentStateMachine — state transitions
import { describe, it, expect } from "vitest";
import {
  createAgentState,
  applySignal,
  applyScreenStatus,
  notifyOutput,
  tick,
  setCliType,
  resetAgentState,
  type AgentState,
} from "../agentStateMachine";

describe("createAgentState", () => {
  it("starts in unknown status", () => {
    const state = createAgentState(1000);
    expect(state.status).toBe("unknown");
    expect(state.cli).toBe("unknown");
    expect(state.blockedMessage).toBeNull();
  });
});

describe("applySignal — blocked", () => {
  it("transitions to blocked on Devin notify signal", () => {
    const state = createAgentState(1000);
    applySignal(state, { kind: "blocked", cli: "devin", message: "Needs input" }, 2000);
    expect(state.status).toBe("blocked");
    expect(state.blockedMessage).toBe("Needs input");
    expect(state.cli).toBe("devin");
  });

  it("blocked is a priority state — fires immediately", () => {
    const state = createAgentState(1000);
    // Even from unknown, blocked fires immediately
    applySignal(state, { kind: "blocked", cli: "devin", message: "Approval" }, 2000);
    expect(state.status).toBe("blocked");
  });
});

describe("applySignal — done", () => {
  it("transitions to done on devin-idle signal", () => {
    const state = createAgentState(1000);
    // First go to working
    notifyOutput(state, 2000);
    state.pendingStatus = null;
    state.status = "working"; // force past debounce for test
    // Then done
    applySignal(state, { kind: "done", cli: "devin" }, 3000);
    expect(state.status).toBe("done");
    expect(state.blockedMessage).toBeNull();
  });
});

describe("applySignal — notify (OSC 777/9)", () => {
  it("transitions to done on 'Devin finished' notify signal", () => {
    const state = createAgentState(1000);
    state.cli = "devin";
    state.status = "working";
    applySignal(state, { kind: "notify", cli: "devin", message: "Devin finished", done: true }, 2000);
    expect(state.status).toBe("done");
    expect(state.blockedMessage).toBeNull();
  });

  it("transitions to blocked on 'Devin needs input' notify signal", () => {
    const state = createAgentState(1000);
    state.cli = "devin";
    state.status = "working";
    applySignal(state, { kind: "notify", cli: "devin", message: "Devin needs input", done: false }, 2000);
    expect(state.status).toBe("blocked");
    expect(state.blockedMessage).toBe("Devin needs input");
  });

  it("transitions to done on 'Devin encountered an error' notify signal", () => {
    const state = createAgentState(1000);
    state.cli = "devin";
    state.status = "working";
    applySignal(state, { kind: "notify", cli: "devin", message: "Devin encountered an error", done: true }, 2000);
    expect(state.status).toBe("done");
  });

  it("blocked → done on 'Devin finished' notify (user answered + Devin finished)", () => {
    const state = createAgentState(1000);
    state.cli = "devin";
    applySignal(state, { kind: "notify", cli: "devin", message: "Devin needs input", done: false }, 2000);
    expect(state.status).toBe("blocked");
    applySignal(state, { kind: "notify", cli: "devin", message: "Devin finished", done: true }, 3000);
    expect(state.status).toBe("done");
  });
});

describe("notifyOutput", () => {
  it("does NOT transition unknown → working when CLI is unknown (plain shell)", () => {
    const state = createAgentState(1000);
    notifyOutput(state, 2000);
    // CLI is unknown — plain shell output should NOT trigger working
    expect(state.status).toBe("unknown");
    expect(state.pendingStatus).toBeNull();
  });

  it("transitions to working (debounced) when CLI is detected and status is unknown", () => {
    const state = createAgentState(1000);
    state.cli = "devin";
    notifyOutput(state, 2000);
    // Debounced — status hasn't changed yet
    expect(state.status).toBe("unknown");
    expect(state.pendingStatus).toBe("working");
  });

  it("transitions idle → working (debounced) when CLI is detected", () => {
    const state = createAgentState(1000);
    state.cli = "devin";
    state.status = "idle";
    notifyOutput(state, 2000);
    // Debounced — status hasn't changed yet
    expect(state.status).toBe("idle");
    expect(state.pendingStatus).toBe("working");
  });

  it("fires debounced working after tick", () => {
    const state = createAgentState(1000);
    state.cli = "devin";
    state.status = "idle";
    notifyOutput(state, 2000);
    expect(state.pendingStatus).toBe("working");
    // Tick after debounce window (500ms)
    tick(state, 2600);
    expect(state.status).toBe("working");
    expect(state.pendingStatus).toBeNull();
  });

  it("does NOT transition blocked → working on output (spinner animation)", () => {
    const state = createAgentState(1000);
    applySignal(state, { kind: "blocked", cli: "devin", message: "Approval" }, 2000);
    expect(state.status).toBe("blocked");
    // PTY output during blocked (spinner, cursor blink) should NOT clear blocked
    notifyOutput(state, 3000);
    expect(state.status).toBe("blocked");
    expect(state.blockedMessage).toBe("Approval");
  });

  it("updates lastOutputAt on every output", () => {
    const state = createAgentState(1000);
    notifyOutput(state, 2000);
    expect(state.lastOutputAt).toBe(2000);
    notifyOutput(state, 2500);
    expect(state.lastOutputAt).toBe(2500);
  });
});

describe("tick — done → idle decay", () => {
  it("decays done → idle after 5s of no output", () => {
    const state = createAgentState(1000);
    // Get to done state
    state.status = "done";
    state.lastOutputAt = 1000;
    // Tick at 4001ms — not yet 5s
    tick(state, 4001);
    expect(state.status).toBe("done");
    // Tick at 6001ms — past 5s
    tick(state, 6001);
    expect(state.status).toBe("idle");
  });

  it("does not decay if output happened recently", () => {
    const state = createAgentState(1000);
    state.status = "done";
    state.lastOutputAt = 5000;
    tick(state, 8000); // only 3s since last output
    expect(state.status).toBe("done");
  });
});

describe("tick — working → done timeout (fallback)", () => {
  it("transitions working → done after 60s of no output (CLI detected)", () => {
    const state = createAgentState(1000);
    state.cli = "devin";
    state.status = "working";
    state.lastOutputAt = 1000;
    // Tick at 61001ms — past 60s timeout (61001 - 1000 = 60001 > 60000)
    tick(state, 61001);
    expect(state.status).toBe("done");
  });

  it("does NOT transition working → done if output happened recently", () => {
    const state = createAgentState(1000);
    state.cli = "devin";
    state.status = "working";
    state.lastOutputAt = 50000;
    tick(state, 60000); // only 10s since last output
    expect(state.status).toBe("working");
  });

  it("does NOT transition working → done if CLI is unknown (plain shell)", () => {
    const state = createAgentState(1000);
    state.cli = "unknown";
    state.status = "working";
    state.lastOutputAt = 1000;
    tick(state, 70000); // 69s since last output, but no CLI
    expect(state.status).toBe("working");
  });

  it("does NOT transition blocked → done (blocked is sticky)", () => {
    const state = createAgentState(1000);
    state.cli = "devin";
    state.status = "blocked";
    state.lastOutputAt = 1000;
    tick(state, 70000);
    expect(state.status).toBe("blocked");
  });
});

describe("tick — debounce firing", () => {
  it("fires pending working after debounce window", () => {
    const state = createAgentState(1000);
    state.cli = "devin";
    state.status = "idle";
    notifyOutput(state, 2000);
    // pendingStatus = working, pendingFireAt = 2500
    tick(state, 2400);
    expect(state.status).toBe("idle"); // not yet
    tick(state, 2600);
    expect(state.status).toBe("working");
  });
});

describe("setCliType", () => {
  it("sets CLI type without changing status", () => {
    const state = createAgentState(1000);
    state.status = "working";
    setCliType(state, "claude-code");
    expect(state.cli).toBe("claude-code");
    expect(state.status).toBe("working");
  });
});

describe("applyScreenStatus", () => {
  it("blocked sets status immediately + stores message", () => {
    const state = createAgentState(1000);
    state.status = "working";
    applyScreenStatus(state, "blocked", "Permission required", 2000);
    expect(state.status).toBe("blocked");
    expect(state.blockedMessage).toBe("Permission required");
    expect(state.pendingStatus).toBeNull();
  });

  it("blocked clears blockedMessage when message is null", () => {
    const state = createAgentState(1000);
    state.blockedMessage = "old message";
    applyScreenStatus(state, "blocked", null, 2000);
    expect(state.status).toBe("blocked");
    expect(state.blockedMessage).toBeNull();
  });

  it("working transitions immediately (no debounce)", () => {
    const state = createAgentState(1000);
    state.status = "idle";
    applyScreenStatus(state, "working", null, 2000);
    // working from screen scrape is immediate — no pendingStatus
    expect(state.status).toBe("working");
    expect(state.pendingStatus).toBeNull();
  });

  it("done transitions immediately + clears blockedMessage", () => {
    const state = createAgentState(1000);
    state.status = "blocked";
    state.blockedMessage = "need input";
    applyScreenStatus(state, "done", null, 2000);
    expect(state.status).toBe("done");
    expect(state.blockedMessage).toBeNull();
    expect(state.pendingStatus).toBeNull();
  });

  it("idle does NOT override blocked", () => {
    const state = createAgentState(1000);
    state.status = "blocked";
    state.blockedMessage = "Permission required";
    applyScreenStatus(state, "idle", null, 2000);
    expect(state.status).toBe("blocked");
    expect(state.blockedMessage).toBe("Permission required");
  });

  it("idle transitions from non-blocked status", () => {
    const state = createAgentState(1000);
    state.status = "working";
    applyScreenStatus(state, "idle", null, 2000);
    expect(state.status).toBe("idle");
    expect(state.pendingStatus).toBeNull();
  });

  it("idle does NOT override done (screen scrape must not skip the 5s done→idle decay)", () => {
    // When Devin finishes, OSC sets status="done" (blue dot). The screen
    // immediately shows the idle placeholder (e.g. "❭ Ask Devin to...").
    // Screen scrape detects "idle" but must NOT override "done" — otherwise
    // the blue dot flashes for <1 tick and instantly turns gray, and the
    // user never sees that Devin finished. The done→idle decay is handled
    // by the tick timer (DONE_TO_IDLE_MS = 5000ms), not by screen scrape.
    const state = createAgentState(1000);
    state.status = "done";
    state.lastOutputAt = 1000;
    applyScreenStatus(state, "idle", null, 2000);
    expect(state.status).toBe("done");
    // After 5s tick decay, done→idle fires
    tick(state, 6001);
    expect(state.status).toBe("idle");
  });

  it("idle DOES transition from working (no done state to protect)", () => {
    // When there's no OSC done signal (CLI doesn't emit OSC), screen scrape
    // detecting the idle placeholder should transition working→idle directly.
    const state = createAgentState(1000);
    state.status = "working";
    applyScreenStatus(state, "idle", null, 2000);
    expect(state.status).toBe("idle");
  });

  it("idle DOES transition from unknown (initial detection)", () => {
    // When CLI is first detected from screen and the screen shows the idle
    // placeholder, status should transition from unknown→idle.
    const state = createAgentState(1000);
    state.status = "unknown";
    applyScreenStatus(state, "idle", null, 2000);
    expect(state.status).toBe("idle");
  });
});

describe("resetAgentState", () => {
  it("resets to initial state", () => {
    const state = createAgentState(1000);
    state.status = "blocked";
    state.cli = "devin";
    state.blockedMessage = "test";
    state.blockedFromOsc = true;
    resetAgentState(state, 5000);
    expect(state.status).toBe("unknown");
    expect(state.cli).toBe("unknown");
    expect(state.blockedMessage).toBeNull();
    expect(state.blockedFromOsc).toBe(false);
  });
});

describe("State transition edge cases", () => {
  it("done → working on new output (new turn)", () => {
    const state = createAgentState(1000);
    state.cli = "devin";
    state.status = "done";
    notifyOutput(state, 2000);
    expect(state.pendingStatus).toBe("working");
    tick(state, 2600);
    expect(state.status).toBe("working");
  });

  it("idle → working on new output", () => {
    const state = createAgentState(1000);
    state.cli = "devin";
    state.status = "idle";
    notifyOutput(state, 2000);
    expect(state.pendingStatus).toBe("working");
    tick(state, 2600);
    expect(state.status).toBe("working");
  });

  it("blocked → done on devin-idle signal", () => {
    const state = createAgentState(1000);
    applySignal(state, { kind: "blocked", cli: "devin", message: "Approval" }, 2000);
    expect(state.status).toBe("blocked");
    applySignal(state, { kind: "done", cli: "devin" }, 3000);
    expect(state.status).toBe("done");
  });
});
