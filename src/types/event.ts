// Tauri event payload types — kept in sync with src-tauri/src/ipc/events.rs
// All Rust payloads are #[serde(rename_all = "camelCase")] so field names
// here match the JSON keys verbatim.

import type { AgentState } from "./agent";
import type { AskUserAnswer, AskUserPromptPayload } from "./tool";

// ---- Phase 1 legacy ----

export interface AgentTokenPayload {
  msgId: string;
  text: string;
}

export interface AgentStatusPayload {
  state: AgentState;
}

export interface AgentErrorPayload {
  msgId: string;
  message: string;
}

export interface AgentDonePayload {
  msgId: string;
}

// ---- Phase 2 new ----

export interface AgentStreamDeltaPayload {
  runId: string;
  msgId: string;
  text: string;
  reasoningDelta?: string;
}

export interface AgentStreamDonePayload {
  runId: string;
  msgId: string;
  reason: string;
  fullText: string;
}

export interface AgentToolRecordPayload {
  runId: string;
  msgId: string;
  record: import("./tool").ToolCallRecord;
}

export interface AgentApprovalRequestPayload {
  approvalId: string;
  runId: string;
  msgId: string;
  toolCallId: string;
  toolName: string;
  arguments: string;
  sensitive: boolean;
}

export interface AgentAskUserPromptPayload {
  askUserId: string;
  runId: string;
  msgId: string;
  toolCallId: string;
  prompt: AskUserPromptPayload;
}

export interface AgentPartialAssistantPayload {
  runId: string;
  msgId: string;
  records: import("./tool").ToolCallRecord[];
  apiMessages: unknown[];
}

export interface AgentToolRejectedPayload {
  runId: string;
  msgId: string;
  toolCallId: string;
  toolName: string;
  reason: string;
}

// ---- Event name constants ----

export const EVT_TOKEN = "agent:token";
export const EVT_STATUS = "agent:status";
export const EVT_ERROR = "agent:error";
export const EVT_DONE = "agent:done";

export const EVT_STREAM_DELTA = "agent:stream_delta";
export const EVT_STREAM_DONE = "agent:stream_done";
export const EVT_TOOL_RECORD = "agent:tool_record";
export const EVT_APPROVAL_REQUEST = "agent:approval_request";
export const EVT_ASK_USER_PROMPT = "agent:ask_user_prompt";
export const EVT_PARTIAL_ASSISTANT = "agent:partial_assistant";
export const EVT_TOOL_REJECTED = "agent:tool_rejected";

// ---- Frontend → backend command payloads ----

export interface ApproveToolArgs {
  approvalId: string;
  allow: boolean;
}

export interface AnswerAskUserArgs {
  askUserId: string;
  response: {
    phase: string;
    answers: Record<string, AskUserAnswer>;
  };
}

// re-export AskUserAnswer so consumers don't have to import twice
export type { AskUserAnswer, AskUserPromptPayload };