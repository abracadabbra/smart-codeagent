export type AgentState =
  | "Idle"
  | "Prepare"
  | "Stream"
  | "Stop"
  | "ToolLoop"
  | "Synthesis"
  | "Plain"
  | "RetryBackoff"
  | "TrimContext";

// Kept in sync with src-tauri/src/agent/mod.rs AgentState enum
export const AGENT_STATES: AgentState[] = [
  "Idle",
  "Prepare",
  "Stream",
  "Stop",
  "ToolLoop",
  "Synthesis",
  "Plain",
  "RetryBackoff",
  "TrimContext",
];