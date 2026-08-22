import { useEffect, useRef, useCallback, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AgentStatus } from "./agentStateMachine";
import type { CliType } from "./oscParser";
import { getBehavior, executeSteps, type BehaviorContext } from "./cliBehavior";
import { useUserIdle } from "./useUserIdle";

// === SECTION 1: types + constants ===

/** 30 秒超时后恢复手机端按钮（B3） */
const REMOTE_ANSWER_TIMEOUT_MS = 30_000;

interface UseAgentRemoteAnswerParams {
  sessionId: string;
  agentStatus: AgentStatus;
  agentCli: CliType;
  agentQuestion: string | null;
  agentOptions: string[] | null;
  // D3: sendToBackend is a ref in TerminalView, pass a stable wrapper
  sendToBackend: (bytes: Uint8Array) => void;
}

interface UseAgentRemoteAnswerResult {
  /** Clear the pushed question ref — call BEFORE local submit to prevent double broadcast (T1/中-2) */
  clearPushedQuestionRef: () => void;
  /** Check if a question has been answered (desktop-side guard, D1/中-2) */
  checkQuestionAnswered: (questionId: string) => Promise<string | null>;
  /** Mark a question as answered from desktop side (中-2: first-answer-wins) */
  markQuestionAnswered: (questionId: string, answer: string) => Promise<boolean>;
  /** The current pushed questionId (null if not pushed or already cleared) */
  currentQuestionId: string | null;
  /** Notify mobile that a question has been resolved (call after local submit completes).
   *  Design doc §4.3.7 step 5: desktop submit must immediately notify mobile. */
  notifyResolved: (questionId: string, answer: string) => Promise<void>;
}

// === SECTION 1 END ===

// === SECTION 2: main hook implementation ===

/**
 * Desktop-side hook for AI CLI remote approval.
 *
 * Responsibilities:
 * 1. When agentStatus → "blocked" AND user is idle → send ipc_tunnel_notify_agent_blocked
 *    (Rust enqueues if no mobile subscriber, flushes on next subscribe)
 * 2. When agentStatus → "working" (agent unblocked) → send ipc_tunnel_notify_agent_resolved
 * 3. Track pushedQuestionIdRef to avoid double-push on effect re-runs (C2/R4)
 * 4. Provide clearPushedQuestionRef for TerminalView handlers to call before local submit (T1/中-2)
 *
 * D2: The actual submit logic (ipc_mark_question_answered + ipc_tunnel_notify_agent_resolved)
 *     is in TerminalView's handlers, NOT in AgentQuestionOverlay.
 * D3: sendToBackend is passed as a stable wrapper (useCallback) because TerminalView
 *     stores it in a ref.
 * D4: cursorPos is not used here — Phase 1 only supports simple option selection.
 */
export function useAgentRemoteAnswer({
  sessionId,
  agentStatus,
  agentCli,
  agentQuestion,
  agentOptions,
  sendToBackend,
}: UseAgentRemoteAnswerParams): UseAgentRemoteAnswerResult {
  const { idle, locked } = useUserIdle();

  // C2/R4: Track the questionId we pushed to mobile, to avoid double-push
  const pushedQuestionIdRef = useRef<string | null>(null);
  // State mirror for exposing to consumers (TerminalView handlers)
  const [currentQuestionId, setCurrentQuestionId] = useState<string | null>(null);
  // Track previous status for transition detection
  const prevStatusRef = useRef<AgentStatus>("unknown");
  // Track the question text when we pushed, to detect question changes
  const pushedQuestionTextRef = useRef<string | null>(null);

  // Generate a stable questionId from session + question text + timestamp
  // (AgentQuestionOverlay doesn't have a questionId prop — D2)
  const generateQuestionId = useCallback((question: string): string => {
    return `${sessionId}:${Date.now()}:${question.slice(0, 50)}`;
  }, [sessionId]);

  // Clear pushed question ref — called by TerminalView handlers before local submit (T1/中-2)
  const clearPushedQuestionRef = useCallback(() => {
    pushedQuestionIdRef.current = null;
    pushedQuestionTextRef.current = null;
    setCurrentQuestionId(null);
  }, []);

  // Check if question answered (desktop-side guard)
  const checkQuestionAnswered = useCallback(async (questionId: string): Promise<string | null> => {
    try {
      const result = await invoke<string | null>("ipc_check_question_answered", { questionId });
      return result;
    } catch {
      return null;
    }
  }, []);

  // Mark question answered from desktop side
  const markQuestionAnswered = useCallback(async (questionId: string, answer: string): Promise<boolean> => {
    try {
      const result = await invoke<boolean>("ipc_mark_question_answered", { questionId, answer });
      return result;
    } catch {
      return false;
    }
  }, []);

  // Notify mobile that a question has been resolved (after local submit completes)
  // Design doc §4.3.7 step 5: desktop submit must immediately notify mobile
  const notifyResolved = useCallback(async (questionId: string, answer: string): Promise<void> => {
    try {
      await invoke("ipc_tunnel_notify_agent_resolved", { sessionId, questionId, answer });
    } catch (e) {
      console.error("useAgentRemoteAnswer: notifyResolved failed", e);
    }
  }, [sessionId]);

  // Main effect: watch agentStatus + idle/locked transitions
  useEffect(() => {
    const prevStatus = prevStatusRef.current;
    prevStatusRef.current = agentStatus;

    // Case 1: AI just became blocked AND user is already idle/locked → push immediately
    if (agentStatus === "blocked" && prevStatus !== "blocked") {
      if (pushedQuestionIdRef.current !== null && pushedQuestionTextRef.current === agentQuestion) {
        return;
      }

      if (!idle && !locked) {
        return;
      }

      const questionId = generateQuestionId(agentQuestion ?? "");
      pushedQuestionIdRef.current = questionId;
      pushedQuestionTextRef.current = agentQuestion;
      setCurrentQuestionId(questionId);

      invoke("ipc_tunnel_notify_agent_blocked", {
        sessionId,
        questionId,
        cli: agentCli,
        question: agentQuestion ?? "",
        options: agentOptions ?? [],
      }).catch((e) => {
        console.error("useAgentRemoteAnswer: notify_agent_blocked failed", e);
      });
      return;
    }

    // Case 2: AI is already blocked, user just became idle/locked → push now
    // (covers: user at desk when AI blocked, then walks away / locks screen)
    if (agentStatus === "blocked" && (idle || locked)) {
      if (pushedQuestionIdRef.current !== null && pushedQuestionTextRef.current === agentQuestion) {
        return; // already pushed this question
      }

      const questionId = generateQuestionId(agentQuestion ?? "");
      pushedQuestionIdRef.current = questionId;
      pushedQuestionTextRef.current = agentQuestion;
      setCurrentQuestionId(questionId);

      invoke("ipc_tunnel_notify_agent_blocked", {
        sessionId,
        questionId,
        cli: agentCli,
        question: agentQuestion ?? "",
        options: agentOptions ?? [],
      }).catch((e) => {
        console.error("useAgentRemoteAnswer: notify_agent_blocked failed (idle transition)", e);
      });
      return;
    }

    // Transition: "blocked" → "working" (agent unblocked, either by local or remote answer)
    if (prevStatus === "blocked" && agentStatus === "working") {
      const pushedQid = pushedQuestionIdRef.current;
      if (pushedQid !== null) {
        // Send NOTIFY(agent_resolved) to mobile so it can close the UI + cancel notification
        invoke("ipc_tunnel_notify_agent_resolved", {
          sessionId,
          questionId: pushedQid,
          answer: "",  // answer may have been provided locally or remotely
        }).catch((e) => {
          console.error("useAgentRemoteAnswer: notify_agent_resolved failed", e);
        });
        // Clear ref so effect doesn't re-fire
        pushedQuestionIdRef.current = null;
        pushedQuestionTextRef.current = null;
        setCurrentQuestionId(null);
      }
    }

    // Transition: "blocked" → "unknown"/"done" (agent exited or terminal closed)
    if (prevStatus === "blocked" && (agentStatus === "unknown" || agentStatus === "done")) {
      const pushedQid = pushedQuestionIdRef.current;
      if (pushedQid !== null) {
        invoke("ipc_tunnel_notify_agent_resolved", {
          sessionId,
          questionId: pushedQid,
          answer: "__dismissed__",
        }).catch(() => {});
        pushedQuestionIdRef.current = null;
        pushedQuestionTextRef.current = null;
        setCurrentQuestionId(null);
      }
    }
  }, [agentStatus, agentQuestion, agentCli, agentOptions, sessionId, idle, locked, generateQuestionId]);

  // E2: Listen for agent_remote_answer events from mobile (via Rust agent_answer_callback)
  // The frontend uses cliBehavior to generate keystrokes + write PTY,
  // then calls ipc_tunnel_notify_agent_resolved (D3: after PTY write completes)
  useEffect(() => {
    const unlisten = listen<{
      question_id: string;
      payload: {
        terminal_id: number;
        session_id: string;
        question_id: string;
        answer: string;
        option_index: number;
        cli: CliType;
        options: string[] | null;
        is_multi_select: boolean;
        is_multi_question: boolean;
      };
    }>("agent_remote_answer", async (event) => {
      const payload = event.payload.payload;
      if (payload.session_id !== sessionId) return;

      const { answer, option_index, cli, options, is_multi_select, is_multi_question, question_id } = payload;
      const ctx: BehaviorContext = {
        options: options,  // E3: from payload, not null
        isMultiSelect: is_multi_select,
        isMultiQuestion: is_multi_question,
        activeTabIndex: 0,  // P7: hardcoded 0 (multi-question not supported in Phase 1)
        totalTabs: 0,
        cursorPos: 0,  // D4: hardcoded 0 (Devin multi-step cursor nav may be inaccurate)
        otherEditing: false,
      };

      try {
        // R4: Clear ref BEFORE await executeSteps to prevent double broadcast
        // (await期间 PTY收到按键 → agentStatus blocked→working → effect重跑 → 再次调resolved)
        clearPushedQuestionRef();

        const result = getBehavior(cli).answer(answer, option_index, ctx);
        await executeSteps(result.steps, sendToBackend);

        // D3: executeSteps完成后才通知手机端 resolved (确保PTY按键已写入)
        await invoke("ipc_tunnel_notify_agent_resolved", {
          sessionId,
          questionId: question_id,
          answer,
        });
      } catch (e) {
        // P4: executeSteps or answer() threw — don't notify resolved (mobile UI stays open)
        // B3: Mobile 30s timeout will re-enable buttons for retry
        console.error("agent_remote_answer executeSteps failed", e);
        // R4: ref already cleared at try start, catch doesn't need to handle ref
      }
    });
    return () => { unlisten.then(fn => fn()); };
  }, [sessionId, sendToBackend, clearPushedQuestionRef]);

  return {
    clearPushedQuestionRef,
    checkQuestionAnswered,
    markQuestionAnswered,
    currentQuestionId,
    notifyResolved,
  };
}

// === SECTION 2 END ===

