import { useEffect, useRef, useCallback, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AgentStatus } from "./agentStateMachine";
import type { CliType } from "./oscParser";
import { getBehavior, executeSteps, type BehaviorContext } from "./cliBehavior";
import { useUserIdle } from "./useUserIdle";
import { useConfigStore } from "../stores/configStore";
import { logTerminalDebug } from "./terminalLogger";

// === SECTION 1: types + constants ===

/** 30 秒超时后恢复手机端按钮（B3） */
const REMOTE_ANSWER_TIMEOUT_MS = 30_000;

/**
 * 检查推送数据是否就绪。
 *
 * 首次检测到 blocked 时屏幕可能还没完全渲染，question 可能为 null。
 * 此时不应推送（不设 ref），等下一个 tick 数据就绪后再推。
 *
 * 注意：不检查兜底文本（如 "OpenCode is asking a question"）和 options 是否为空。
 * 原因：multi-question 场景（P7 已知限制）下 questionExtractor 一直返回兜底文本、
 * optionsExtractor 一直返回 null，这是提取器限制不是屏幕未渲染。
 * 如果拦截兜底文本会导致问题永远不入 pending_questions 队列，补发测试失败。
 * 兜底文本也是有效的推送内容——用户看到通知至少知道 AI blocked 了。
 */
function isPushDataReady(
  _cli: CliType,
  question: string | null,
  _options: string[] | null,
): boolean {
  // question 只要有文本就行（包括兜底文本），null/空表示屏幕还没渲染
  return !!question && question !== "";
}

interface UseAgentRemoteAnswerParams {
  sessionId: string;
  agentStatus: AgentStatus;
  agentCli: CliType;
  agentQuestion: string | null;
  agentOptions: string[] | null;
  /** True if the current blocked dialog is multi-question (has Confirm tab). */
  isMultiQuestion: boolean;
  /** Active tab index in multi-question dialog (-1 if not multi-question). */
  activeTabIndex: number;
  /** Total number of tabs in multi-question dialog (0 if not multi-question). */
  totalTabs: number;
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
  isMultiQuestion,
  activeTabIndex,
  totalTabs,
  sendToBackend,
}: UseAgentRemoteAnswerParams): UseAgentRemoteAnswerResult {
  const devIdleThresholdSecs = useConfigStore((s) => s.config?.general.dev_idle_threshold_secs ?? 0);
  const { idle, locked } = useUserIdle(devIdleThresholdSecs || undefined);

  // C2/R4: Track the questionId we pushed to mobile, to avoid double-push
  const pushedQuestionIdRef = useRef<string | null>(null);
  // State mirror for exposing to consumers (TerminalView handlers)
  const [currentQuestionId, setCurrentQuestionId] = useState<string | null>(null);
  // Track previous status for transition detection
  const prevStatusRef = useRef<AgentStatus>("unknown");
  const prevIdleRef = useRef(false);
  const prevLockedRef = useRef(false);
  // Track the question text when we pushed, to detect question changes
  const pushedQuestionTextRef = useRef<string | null>(null);
  // Track the tab index we pushed, for multi-question re-push on tab change
  const pushedTabIndexRef = useRef<number>(-1);

  // Generate a stable questionId from session + question text + timestamp
  // (AgentQuestionOverlay doesn't have a questionId prop — D2)
  const generateQuestionId = useCallback((question: string): string => {
    return `${sessionId}:${Date.now()}:${question.slice(0, 50)}`;
  }, [sessionId]);

  // Clear pushed question ref — called by TerminalView handlers before local submit (T1/中-2)
  // Also sends agent_resolved to mobile so it can cancel the notification.
  const clearPushedQuestionRef = useCallback(() => {
    const pushedQid = pushedQuestionIdRef.current;
    if (pushedQid !== null) {
      console.log(`[REMOTE_PUSH] clearPushedQuestionRef: sending agent_resolved for questionId=${pushedQid}`);
      invoke("ipc_tunnel_notify_agent_resolved", {
        sessionId,
        questionId: pushedQid,
        answer: "",
      }).then(() => {
        console.log("[REMOTE_PUSH] clearPushedQuestionRef: agent_resolved invoke SUCCESS");
      }).catch((e) => {
        console.error("[REMOTE_PUSH] clearPushedQuestionRef: agent_resolved invoke FAILED:", e);
      });
    }
    pushedQuestionIdRef.current = null;
    pushedQuestionTextRef.current = null;
    pushedTabIndexRef.current = -1;
    setCurrentQuestionId(null);
  }, [sessionId]);

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

    // Only log when something relevant changes (not every tick)
    if (agentStatus !== prevStatus || idle !== prevIdleRef.current || locked !== prevLockedRef.current) {
      console.log(`[REMOTE_PUSH] session=${sessionId} status=${agentStatus} prev=${prevStatus} idle=${idle} locked=${locked} pushedQ=${pushedQuestionIdRef.current}`);
      prevIdleRef.current = idle;
      prevLockedRef.current = locked;
    }

    // Case 1: AI just became blocked AND user is already idle/locked → push immediately
    if (agentStatus === "blocked" && prevStatus !== "blocked") {
      // 边界-2: blocked 期间 question/options 变化不重推。
      // 只要本次 blocked 会话已推过（ref != null），就不再推，
      // 即使 question 文本因屏幕抓取抖动而变化（如 "errupted" 片段）。
      if (pushedQuestionIdRef.current !== null) {
        console.log("[REMOTE_PUSH] Case1 skip: already pushed for this blocked session");
        return;
      }

      if (!idle && !locked) {
        console.log("[REMOTE_PUSH] Case1 skip: user not idle/locked");
        return;
      }

      // 数据就绪检查：首次检测到 blocked 时屏幕可能还没完全渲染，
      // questionExtractor 会返回兜底文本（如 "OpenCode is asking a question"），
      // options 也可能还没抓到。此时跳过推送（不设 ref），
      // 等下一个 tick 数据就绪后由 Case 2 补推。
      if (!isPushDataReady(agentCli, agentQuestion, agentOptions)) {
        console.log(`[REMOTE_PUSH] Case1 skip: data not ready (question=${JSON.stringify(agentQuestion)} options=${agentOptions?.length ?? 0})`);
        return;
      }

      const questionId = generateQuestionId(agentQuestion ?? "");
      pushedQuestionIdRef.current = questionId;
      pushedQuestionTextRef.current = agentQuestion;
      pushedTabIndexRef.current = isMultiQuestion ? activeTabIndex : -1;
      setCurrentQuestionId(questionId);

      console.log(`[REMOTE_PUSH] Case1 PUSHING: questionId=${questionId} cli=${agentCli} question=${agentQuestion} multiQ=${isMultiQuestion} tab=${activeTabIndex}/${totalTabs} options=${agentOptions?.length ?? 0}`);
      logTerminalDebug(sessionId, `[REMOTE_PUSH] Case1 PUSHING: questionId=${questionId} cli=${agentCli} question=${agentQuestion} multiQ=${isMultiQuestion} tab=${activeTabIndex}/${totalTabs} options=${JSON.stringify(agentOptions)}`);
      invoke("ipc_tunnel_notify_agent_blocked", {
        sessionId,
        questionId,
        cli: agentCli,
        question: agentQuestion ?? "",
        options: agentOptions ?? [],
        isMultiQuestion,
        activeTabIndex,
        totalTabs,
      }).then(() => {
        console.log("[REMOTE_PUSH] Case1 invoke SUCCESS");
      }).catch((e) => {
        console.error("[REMOTE_PUSH] Case1 invoke FAILED:", e);
      });
      return;
    }

    // Case 1.5: Multi-question mode — tab changed (user answered one question,
    // agent advanced to next tab). Re-push with new question text + options.
    // The agent stays "blocked" throughout the multi-question dialog, so the
    // normal Case 1/2 dedup would prevent re-pushing. We detect tab changes
    // and explicitly re-push.
    if (agentStatus === "blocked" && isMultiQuestion && pushedQuestionIdRef.current !== null) {
      if (activeTabIndex !== pushedTabIndexRef.current && activeTabIndex >= 0) {
        // Tab changed — send agent_resolved for old question, then push new one
        const oldQid = pushedQuestionIdRef.current;
        console.log(`[REMOTE_PUSH] Case1.5: tab changed ${pushedTabIndexRef.current}→${activeTabIndex}, re-pushing`);
        invoke("ipc_tunnel_notify_agent_resolved", {
          sessionId,
          questionId: oldQid,
          answer: "",
        }).catch(() => {});

        // Data readiness check for new question
        if (!isPushDataReady(agentCli, agentQuestion, agentOptions)) {
          console.log(`[REMOTE_PUSH] Case1.5 skip: data not ready for new tab`);
          return;
        }

        const questionId = generateQuestionId(agentQuestion ?? "");
        pushedQuestionIdRef.current = questionId;
        pushedQuestionTextRef.current = agentQuestion;
        pushedTabIndexRef.current = activeTabIndex;
        setCurrentQuestionId(questionId);

        console.log(`[REMOTE_PUSH] Case1.5 PUSHING: questionId=${questionId} question=${agentQuestion} tab=${activeTabIndex}/${totalTabs}`);
        invoke("ipc_tunnel_notify_agent_blocked", {
          sessionId,
          questionId,
          cli: agentCli,
          question: agentQuestion ?? "",
          options: agentOptions ?? [],
          isMultiQuestion,
          activeTabIndex,
          totalTabs,
        }).catch((e) => {
          console.error("[REMOTE_PUSH] Case1.5 invoke FAILED:", e);
        });
        return;
      }
    }

    // Case 2: AI is already blocked, user just became idle/locked → push now
    // (covers: user at desk when AI blocked, then walks away / locks screen)
    // Also handles delayed push when Case 1 skipped due to data not ready.
    if (agentStatus === "blocked" && (idle || locked)) {
      // 边界-2: blocked 期间 question/options 变化不重推。
      // 只要本次 blocked 会话已推过（ref != null），就不再推，
      // 即使 question 文本因屏幕抓取抖动而变化（如 "errupted" 片段）。
      if (pushedQuestionIdRef.current !== null) {
        return; // already pushed for this blocked session
      }

      // 数据就绪检查（同 Case 1）：数据未就绪时跳过，不设 ref，
      // 等下一个 tick 数据就绪后再次进入 Case 2 补推。
      if (!isPushDataReady(agentCli, agentQuestion, agentOptions)) {
        return;
      }

      const questionId = generateQuestionId(agentQuestion ?? "");
      pushedQuestionIdRef.current = questionId;
      pushedQuestionTextRef.current = agentQuestion;
      pushedTabIndexRef.current = isMultiQuestion ? activeTabIndex : -1;
      setCurrentQuestionId(questionId);

      invoke("ipc_tunnel_notify_agent_blocked", {
        sessionId,
        questionId,
        cli: agentCli,
        question: agentQuestion ?? "",
        options: agentOptions ?? [],
        isMultiQuestion,
        activeTabIndex,
        totalTabs,
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
        console.log(`[REMOTE_PUSH] blocked→working, sending agent_resolved for questionId=${pushedQid}`);
        invoke("ipc_tunnel_notify_agent_resolved", {
          sessionId,
          questionId: pushedQid,
          answer: "",  // answer may have been provided locally or remotely
        }).then(() => {
          console.log("[REMOTE_PUSH] agent_resolved invoke SUCCESS");
        }).catch((e) => {
          console.error("[REMOTE_PUSH] agent_resolved invoke FAILED:", e);
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
        console.log(`[REMOTE_PUSH] blocked→${agentStatus}, sending agent_resolved(dismissed) for questionId=${pushedQid}`);
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
  }, [agentStatus, agentQuestion, agentCli, agentOptions, isMultiQuestion, activeTabIndex, totalTabs, sessionId, idle, locked, generateQuestionId]);

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
        active_tab_index: number;
        total_tabs: number;
      };
    }>("agent_remote_answer", async (event) => {
      const payload = event.payload.payload;
      if (payload.session_id !== sessionId) return;

      const { answer, option_index, cli, options, is_multi_select, is_multi_question, active_tab_index, total_tabs, question_id } = payload;

      // FP9: __answered__ means mobile autonomously answered (wrote PTY directly).
      // Desktop should just close the overlay — no cliBehavior, no PTY write, no double keypress.
      // The Rust side already broadcasted NOTIFY(agent_resolved) + QUESTION_RESOLVED to mobile.
      if (answer === "__answered__") {
        clearPushedQuestionRef();
        // Notify desktop overlay to close (agent_resolved event triggers overlay dismissal)
        try {
          await invoke("ipc_tunnel_notify_agent_resolved", {
            sessionId,
            questionId: question_id,
            answer,
          });
        } catch (e) {
          console.error("agent_remote_answer __answered__ notify resolved failed", e);
        }
        return;
      }

      const ctx: BehaviorContext = {
        options: options,  // E3: from payload, not null
        isMultiSelect: is_multi_select,
        isMultiQuestion: is_multi_question,
        activeTabIndex: active_tab_index,  // from payload (multi-question tab tracking)
        totalTabs: total_tabs,
        cursorPos: 0,  // D4: hardcoded 0 (Devin multi-step cursor nav may be inaccurate)
        otherEditing: false,
      };

      try {
        // Special navigation/confirm answers for multi-question mode
        if (answer === "__nav_prev__") {
          const result = getBehavior(cli).prevQuestion(ctx);
          await executeSteps(result.steps, sendToBackend);
          return;  // don't notify resolved — agent still blocked
        }
        if (answer === "__nav_next__") {
          const result = getBehavior(cli).nextQuestion(ctx);
          await executeSteps(result.steps, sendToBackend);
          return;  // don't notify resolved — agent still blocked
        }
        if (answer === "__confirm__") {
          const result = getBehavior(cli).confirm(false, ctx);
          await executeSteps(result.steps, sendToBackend);
          // Confirm submits all answers → notify resolved
          await invoke("ipc_tunnel_notify_agent_resolved", {
            sessionId,
            questionId: question_id,
            answer,
          });
          return;
        }

        // Normal answer: clear ref BEFORE await executeSteps to prevent double broadcast
        // (await期间 PTY收到按键 → agentStatus blocked→working → effect重跑 → 再次调resolved)
        clearPushedQuestionRef();

        const result = getBehavior(cli).answer(answer, option_index, ctx);
        await executeSteps(result.steps, sendToBackend);

        // D3: executeSteps完成后才通知手机端 resolved (确保PTY按键已写入)
        // For multi-question: don't notify resolved — agent still blocked on next tab.
        // Case 1.5 will send agent_resolved + new agent_blocked when tab changes.
        if (!is_multi_question) {
          await invoke("ipc_tunnel_notify_agent_resolved", {
            sessionId,
            questionId: question_id,
            answer,
          });
        }
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

