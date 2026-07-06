// Tauri event payload types — kept in sync with src-tauri/src/ipc/events.rs
// All Rust payloads are #[serde(rename_all = "camelCase")] so field names
// here match the JSON keys verbatim.

import type { AgentState } from "./agent";
import type { Conversation } from "./session";
import type { AskUserAnswer, AskUserPromptPayload, ToolCallRecord } from "./tool";

// ---- Phase 1 legacy (Phase 3.2: all payloads gain conversationId) ----

export interface AgentTokenPayload {
  conversationId: string;
  msgId: string;
  text: string;
}

export interface AgentStatusPayload {
  conversationId: string;
  state: AgentState;
}

export interface AgentErrorPayload {
  conversationId: string;
  msgId: string;
  message: string;
}

export interface AgentDonePayload {
  conversationId: string;
  msgId: string;
}

// ---- Phase 2 new (Phase 3.2: all payloads gain conversationId) ----

export interface AgentStreamDeltaPayload {
  conversationId: string;
  runId: string;
  msgId: string;
  text: string;
  reasoningDelta?: string;
}

export interface AgentStreamDonePayload {
  conversationId: string;
  runId: string;
  msgId: string;
  reason: string;
  fullText: string;
}

export interface AgentToolRecordPayload {
  conversationId: string;
  runId: string;
  msgId: string;
  record: ToolCallRecord;
}

export interface AgentApprovalRequestPayload {
  conversationId: string;
  approvalId: string;
  runId: string;
  msgId: string;
  toolCallId: string;
  toolName: string;
  arguments: string;
  sensitive: boolean;
}

export interface AgentAskUserPromptPayload {
  conversationId: string;
  askUserId: string;
  runId: string;
  msgId: string;
  toolCallId: string;
  prompt: AskUserPromptPayload;
}

export interface AgentPartialAssistantPayload {
  conversationId: string;
  runId: string;
  msgId: string;
  records: ToolCallRecord[];
  apiMessages: unknown[];
}

export interface AgentToolRejectedPayload {
  conversationId: string;
  runId: string;
  msgId: string;
  toolCallId: string;
  toolName: string;
  reason: string;
}

// ---- Phase 3.2: session events ----

export interface SessionCreatedPayload {
  conversation: Conversation;
}

export interface SessionUpdatedPayload {
  conversation: Conversation;
}

export interface SessionDeletedPayload {
  conversationId: string;
}

export interface SessionStatePayload {
  conversationId: string;
  state: AgentState;
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

// Phase 3.2 session events
export const EVT_SESSION_CREATED = "session:created";
export const EVT_SESSION_UPDATED = "session:updated";
export const EVT_SESSION_DELETED = "session:deleted";
export const EVT_SESSION_STATE = "session:state";

// ---- Frontend → backend command payloads ----

// Phase 3.2: send_message now takes conversationId + runId (backend generates messageId + generation)
export interface SendMessageArgs {
  conversationId: string;
  text: string;
  runId: string;
}

// Phase 3.2: all approval/ask_user commands carry conversationId for per-session routing
export interface ApproveToolArgs {
  conversationId: string;
  approvalId: string;
  allow: boolean;
}

export interface AnswerAskUserArgs {
  conversationId: string;
  askUserId: string;
  response: {
    phase: string;
    answers: Record<string, AskUserAnswer>;
  };
}

export interface CancelRunArgs {
  conversationId: string;
}

// re-export AskUserAnswer so consumers don't have to import twice
export type { AskUserAnswer, AskUserPromptPayload };
