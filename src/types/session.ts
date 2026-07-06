// Session management types — kept in sync with src-tauri/src/session/types.rs
// All Rust payloads are #[serde(rename_all = "camelCase")].

import type { ToolCallRecord } from "./tool";

export interface Conversation {
  id: string;
  title: string;
  createdAt: number;
  updatedAt: number;
  pinned: boolean;
  messageCount: number;
}

export interface ConversationListItem {
  id: string;
  title: string;
  preview: string;
  createdAt: number;
  updatedAt: number;
  pinned: boolean;
  messageCount: number;
}

export type ChatRole = "user" | "assistant" | "tool";

export interface OpenAiToolCall {
  id: string;
  type: "function";
  function: { name: string; arguments: string };
}

export interface ChatMessage {
  id: string;
  role: ChatRole;
  content?: string;
  toolCalls?: OpenAiToolCall[];
  toolCallId?: string;
  toolRecords?: ToolCallRecord[];
  createdAt: number;
}

export interface SessionMessagesPage {
  messages: ChatMessage[];
  total: number;
  hasMore: boolean;
}
