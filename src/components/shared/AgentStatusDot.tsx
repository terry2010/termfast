// AgentStatusDot — colored status indicator for terminal tab labels
//
// Shows a small indicator next to the tab label showing the AI CLI's state:
//   🔄 spinner = working (AI is generating output) — animated rotating arc
//   🔴 red     = blocked (AI needs user input / approval) — pulsing dot
//   🔵 blue    = done    (AI finished, waiting for next prompt)
//   ⚪ gray    = idle    (no recent activity)
//   (hidden)  = unknown (no AI CLI detected yet)

import { memo } from "react";
import type { AgentStatus } from "@/hooks/agentStateMachine";

interface AgentStatusDotProps {
  status: AgentStatus;
  /** Size in pixels (default 8). */
  size?: number;
}

const STATUS_COLORS: Record<AgentStatus, string> = {
  unknown: "bg-gray-400",
  idle: "bg-gray-400",
  working: "bg-yellow-400",
  blocked: "bg-red-500",
  done: "bg-blue-400",
};

function AgentStatusDotImpl({ status, size = 8 }: AgentStatusDotProps) {
  // Don't render anything for unknown status (no AI CLI detected yet)
  if (status === "unknown") return null;

  // Working: animated spinner (rotating arc) — much more visible than a dot
  if (status === "working") {
    return (
      <span
        className="inline-block animate-spin rounded-full border-2 border-yellow-400 border-t-transparent"
        style={{ width: size + 2, height: size + 2, flexShrink: 0 }}
        aria-label="agent-status-working"
      />
    );
  }

  const color = STATUS_COLORS[status];
  // Blocked: pulsing red dot to draw attention
  const pulse = status === "blocked";

  return (
    <span
      className={`inline-block rounded-full ${color} ${pulse ? "animate-pulse" : ""}`}
      style={{ width: size, height: size, flexShrink: 0 }}
      aria-label={`agent-status-${status}`}
    />
  );
}

export const AgentStatusDot = memo(AgentStatusDotImpl);
