// overlayReset — pure functions for agent overlay dismissed-state reset logic
//
// When an AI CLI is blocked (needs user input), an overlay shows question +
// answer options. After the user clicks an answer, the overlay is dismissed.
// The dismissed state must reset when:
//   1. The agent status leaves "blocked" (normal: agent started processing)
//   2. A new question appears while still "blocked" (multi-question dialogs:
//      OpenCode shows question 1, then question 2, etc. without leaving blocked)

import type { AgentStatus } from "./agentStateMachine";

/**
 * Determine whether the overlay dismissed state should reset to false.
 *
 * @param prevStatus   the previous agent status
 * @param currStatus   the current agent status
 * @param prevQuestion the previous question text (null if none)
 * @param currQuestion the current question text (null if none)
 * @returns true if the overlay should be re-shown (dismissed → false)
 */
export function shouldResetOverlay(
  prevStatus: AgentStatus,
  currStatus: AgentStatus,
  prevQuestion: string | null,
  currQuestion: string | null,
): boolean {
  // Case 1: status left "blocked" → always reset
  if (prevStatus === "blocked" && currStatus !== "blocked") {
    return true;
  }
  // Case 2: still blocked but question changed → reset for new question
  if (currStatus === "blocked" && currQuestion !== null && currQuestion !== prevQuestion) {
    return true;
  }
  return false;
}
