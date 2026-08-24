// Unit tests for useAgentRemoteAnswer — 边界-2: blocked 期间 question 变化不重推
import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useAgentRemoteAnswer } from "@/hooks/useAgentRemoteAnswer";

// Mock @tauri-apps/api/core (invoke) — returns resolved Promise
const mockInvoke = vi.fn((..._args: any[]) => Promise.resolve() as Promise<unknown>);
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: any[]) => mockInvoke(...(args as [string, Record<string, unknown>])),
}));

// Mock @tauri-apps/api/event (listen) — returns unlisten fn
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

// Mock useUserIdle — controllable idle/locked
let mockIdle = false;
let mockLocked = false;
vi.mock("@/hooks/useUserIdle", () => ({
  useUserIdle: () => ({ idle: mockIdle, locked: mockLocked }),
}));

// Mock useConfigStore
vi.mock("@/stores/configStore", () => ({
  useConfigStore: (selector: (s: any) => any) =>
    selector({ config: { general: { dev_idle_threshold_secs: 60 } } }),
}));

// Mock cliBehavior (used in agent_remote_answer listener, not in push logic)
vi.mock("@/hooks/cliBehavior", () => ({
  getBehavior: () => ({ answer: () => ({ steps: [] }) }),
  executeSteps: vi.fn(() => Promise.resolve()),
}));

const baseParams = {
  sessionId: "test-session",
  agentCli: "opencode" as const,
  agentOptions: ["1. Option A", "2. Option B"],
  isMultiQuestion: false,
  activeTabIndex: -1,
  totalTabs: 0,
  sendToBackend: vi.fn(),
};

function renderHookWithStatus(status: string, question: string | null) {
  return renderHook(
    (props: any) =>
      useAgentRemoteAnswer({
        ...baseParams,
        agentStatus: props.status,
        agentQuestion: props.question,
        agentOptions: props.options !== undefined ? props.options : baseParams.agentOptions,
        isMultiQuestion: props.isMultiQuestion ?? false,
        activeTabIndex: props.activeTabIndex ?? -1,
        totalTabs: props.totalTabs ?? 0,
        sendToBackend: props.sendToBackend ?? baseParams.sendToBackend,
      }),
    {
      initialProps: {
        status,
        question,
        options: baseParams.agentOptions as string[] | null,
        sendToBackend: baseParams.sendToBackend,
        isMultiQuestion: false,
        activeTabIndex: -1,
        totalTabs: 0,
      } as any,
    },
  );
}

describe("useAgentRemoteAnswer — push dedup (边界-2)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockIdle = false;
    mockLocked = false;
  });

  it("Case1: entering blocked + idle → pushes exactly once", () => {
    mockIdle = true;
    const { result, rerender } = renderHookWithStatus("working", null);

    // Transition to blocked
    rerender({ status: "blocked", question: "选择框架", options: baseParams.agentOptions, sendToBackend: baseParams.sendToBackend });

    const blockedCalls = mockInvoke.mock.calls.filter(
      (c: any[]) => c[0] === "ipc_tunnel_notify_agent_blocked",
    );
    expect(blockedCalls).toHaveLength(1);
    expect(blockedCalls[0][1].question).toBe("选择框架");
  });

  it("Case2: blocked + question text changes (screen scrape jitter) → does NOT re-push", () => {
    mockIdle = true;
    const { result, rerender } = renderHookWithStatus("working", null);

    // Enter blocked with correct question
    rerender({ status: "blocked", question: "针对「有卡顿」，想怎么优化？", options: baseParams.agentOptions, sendToBackend: baseParams.sendToBackend });
    // Screen scrape jitter: question becomes "errupted" fragment
    rerender({ status: "blocked", question: "errupted", options: baseParams.agentOptions, sendToBackend: baseParams.sendToBackend });
    // Screen scrape recovers: question back to correct
    rerender({ status: "blocked", question: "针对「有卡顿」，想怎么优化？", options: baseParams.agentOptions, sendToBackend: baseParams.sendToBackend });

    const blockedCalls = mockInvoke.mock.calls.filter(
      (c: any[]) => c[0] === "ipc_tunnel_notify_agent_blocked",
    );
    expect(blockedCalls).toHaveLength(1);
  });

  it("Case2: user at desk when blocked, then walks away → pushes once", () => {
    mockIdle = false;
    const { result, rerender } = renderHookWithStatus("working", null);

    // Enter blocked while user at desk (not idle) → Case1 skips
    rerender({ status: "blocked", question: "选择框架", options: baseParams.agentOptions, sendToBackend: baseParams.sendToBackend });
    expect(mockInvoke).not.toHaveBeenCalled();

    // User walks away → becomes idle
    mockIdle = true;
    rerender({ status: "blocked", question: "选择框架", options: baseParams.agentOptions, sendToBackend: baseParams.sendToBackend });

    const blockedCalls = mockInvoke.mock.calls.filter(
      (c: any[]) => c[0] === "ipc_tunnel_notify_agent_blocked",
    );
    expect(blockedCalls).toHaveLength(1);
  });

  it("blocked→working clears ref → next blocked can push again", () => {
    mockIdle = true;
    const { result, rerender } = renderHookWithStatus("working", null);

    // First blocked → push
    rerender({ status: "blocked", question: "Q1", options: baseParams.agentOptions, sendToBackend: baseParams.sendToBackend });
    // Resolve → working
    rerender({ status: "working", question: null, options: baseParams.agentOptions, sendToBackend: baseParams.sendToBackend });
    // Second blocked → push again
    rerender({ status: "blocked", question: "Q2", options: baseParams.agentOptions, sendToBackend: baseParams.sendToBackend });

    const blockedCalls = mockInvoke.mock.calls.filter(
      (c: any[]) => c[0] === "ipc_tunnel_notify_agent_blocked",
    );
    expect(blockedCalls).toHaveLength(2);
    expect(blockedCalls[0][1].question).toBe("Q1");
    expect(blockedCalls[1][1].question).toBe("Q2");
  });

  it("clearPushedQuestionRef allows next push within same blocked session (E-5.1 multi-question)", () => {
    mockIdle = true;
    const { result, rerender } = renderHookWithStatus("working", null);

    // Enter blocked → push Q1
    rerender({ status: "blocked", question: "Q1", options: baseParams.agentOptions, sendToBackend: baseParams.sendToBackend });
    expect(mockInvoke.mock.calls.filter((c: any[]) => c[0] === "ipc_tunnel_notify_agent_blocked")).toHaveLength(1);

    // Simulate clearPushedQuestionRef (called before local/remote answer submit)
    act(() => {
      result.current.clearPushedQuestionRef();
    });

    // AI shows Q2 while still blocked (multi-question)
    rerender({ status: "blocked", question: "Q2", options: baseParams.agentOptions, sendToBackend: baseParams.sendToBackend });

    const blockedCalls = mockInvoke.mock.calls.filter(
      (c: any[]) => c[0] === "ipc_tunnel_notify_agent_blocked",
    );
    expect(blockedCalls).toHaveLength(2);
    expect(blockedCalls[1][1].question).toBe("Q2");
  });

  it("Case1→Case2: first blocked has null question (screen not rendered) → delays push until question available", () => {
    mockIdle = true;
    const { result, rerender } = renderHookWithStatus("working", null);

    // First blocked detection: screen mid-redraw, question not yet extracted
    rerender({ status: "blocked", question: null, options: null, sendToBackend: baseParams.sendToBackend });
    expect(mockInvoke).not.toHaveBeenCalled();

    // Next tick: screen rendered, question available (options may still be null in multi-question)
    rerender({ status: "blocked", question: "针对「有卡顿」，想怎么优化？", options: baseParams.agentOptions, sendToBackend: baseParams.sendToBackend });

    const blockedCalls = mockInvoke.mock.calls.filter(
      (c: any[]) => c[0] === "ipc_tunnel_notify_agent_blocked",
    );
    expect(blockedCalls).toHaveLength(1);
    expect(blockedCalls[0][1].question).toBe("针对「有卡顿」，想怎么优化？");
    expect(blockedCalls[0][1].options).toEqual(baseParams.agentOptions);
  });

  it("multi-question: fallback question + null options (P7 extractor limit) → pushes immediately", () => {
    mockIdle = true;
    const { result, rerender } = renderHookWithStatus("working", null);

    // Multi-question scenario: questionExtractor returns fallback, optionsExtractor returns null
    // This is a known extractor limit (P7), not screen-not-rendered.
    // Must push immediately so the question enters pending_questions for reconnect补发.
    rerender({ status: "blocked", question: "OpenCode is asking a question", options: null, sendToBackend: baseParams.sendToBackend });

    const blockedCalls = mockInvoke.mock.calls.filter(
      (c: any[]) => c[0] === "ipc_tunnel_notify_agent_blocked",
    );
    expect(blockedCalls).toHaveLength(1);
    expect(blockedCalls[0][1].question).toBe("OpenCode is asking a question");
    expect(blockedCalls[0][1].options).toEqual([]);
  });

  it("claude-code text mode: null options is valid → pushes with question only", () => {
    mockIdle = true;
    const { result, rerender } = renderHook(
      (props: any) =>
        useAgentRemoteAnswer({
          ...baseParams,
          agentCli: "claude-code" as const,
          agentStatus: props.status,
          agentQuestion: props.question,
          agentOptions: props.options,
          sendToBackend: baseParams.sendToBackend,
        }),
      {
        initialProps: { status: "working" as string, question: null as string | null, options: null as string[] | null },
      },
    );

    // Enter blocked with question but no options (text mode) → should push
    rerender({ status: "blocked", question: "Do you want to proceed?", options: null });

    const blockedCalls = mockInvoke.mock.calls.filter(
      (c: any[]) => c[0] === "ipc_tunnel_notify_agent_blocked",
    );
    expect(blockedCalls).toHaveLength(1);
    expect(blockedCalls[0][1].question).toBe("Do you want to proceed?");
    expect(blockedCalls[0][1].options).toEqual([]);
  });

  it("multi-question: tab change re-pushes new question (Case 1.5)", () => {
    mockIdle = true;
    const { result, rerender } = renderHookWithStatus("working", null);

    // Enter blocked with multi-question dialog, tab 0 of 3
    rerender({
      status: "blocked",
      question: "选择编程语言",
      options: ["1. Rust", "2. Go"],
      isMultiQuestion: true,
      activeTabIndex: 0,
      totalTabs: 3,
      sendToBackend: baseParams.sendToBackend,
    });

    let blockedCalls = mockInvoke.mock.calls.filter(
      (c: any[]) => c[0] === "ipc_tunnel_notify_agent_blocked",
    );
    expect(blockedCalls).toHaveLength(1);
    expect(blockedCalls[0][1].isMultiQuestion).toBe(true);
    expect(blockedCalls[0][1].activeTabIndex).toBe(0);
    expect(blockedCalls[0][1].totalTabs).toBe(3);

    // User answers Q1 → agent advances to tab 1 (still blocked)
    rerender({
      status: "blocked",
      question: "选择测试框架",
      options: ["1. Vitest", "2. Jest"],
      isMultiQuestion: true,
      activeTabIndex: 1,
      totalTabs: 3,
      sendToBackend: baseParams.sendToBackend,
    });

    blockedCalls = mockInvoke.mock.calls.filter(
      (c: any[]) => c[0] === "ipc_tunnel_notify_agent_blocked",
    );
    expect(blockedCalls).toHaveLength(2);
    expect(blockedCalls[1][1].question).toBe("选择测试框架");
    expect(blockedCalls[1][1].activeTabIndex).toBe(1);

    // agent_resolved should have been sent for the old question
    const resolvedCalls = mockInvoke.mock.calls.filter(
      (c: any[]) => c[0] === "ipc_tunnel_notify_agent_resolved",
    );
    expect(resolvedCalls).toHaveLength(1);
  });

  it("multi-question: same tab (screen jitter) does NOT re-push", () => {
    mockIdle = true;
    const { result, rerender } = renderHookWithStatus("working", null);

    // Enter blocked with multi-question, tab 0
    rerender({
      status: "blocked",
      question: "选择编程语言",
      options: ["1. Rust", "2. Go"],
      isMultiQuestion: true,
      activeTabIndex: 0,
      totalTabs: 3,
      sendToBackend: baseParams.sendToBackend,
    });

    // Same tab, question text jitters (screen scrape artifact)
    rerender({
      status: "blocked",
      question: "errupted",
      options: ["1. Rust", "2. Go"],
      isMultiQuestion: true,
      activeTabIndex: 0,
      totalTabs: 3,
      sendToBackend: baseParams.sendToBackend,
    });

    const blockedCalls = mockInvoke.mock.calls.filter(
      (c: any[]) => c[0] === "ipc_tunnel_notify_agent_blocked",
    );
    expect(blockedCalls).toHaveLength(1);
  });
});
