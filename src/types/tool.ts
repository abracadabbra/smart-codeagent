// Tool system types — kept in sync with src-tauri/src/agent/tools/mod.rs
// (ChatToolDefinition, ToolCallRecord, ToolCallStatus, AskUser types).

export interface AskUserOption {
  id: string;
  label: string;
  description?: string;
}

export interface AskUserQuestion {
  id: string;
  prompt: string;
  options: AskUserOption[];
  allowMultiple: boolean;
  allowCustom: boolean;
}

export interface AskUserPromptPayload {
  title?: string;
  questions: AskUserQuestion[];
}

export interface AskUserAnswer {
  selectedOptionIds: string[];
  customText?: string;
}

export type ToolCallStatus =
  | "Pending"
  | "Running"
  | "Success"
  | "Error"
  | "Cancelled"
  | "Skipped";

export interface ToolCallRecord {
  id: string;
  name: string;
  source: string;
  serverId?: string;
  arguments: string;
  status: ToolCallStatus;
  resultPreview?: string;
  error?: string;
  durationMs?: number;
  startedAt?: number;
  completedAt?: number;
  round: number;
  sensitive: boolean;
  artifacts: string[];
  structuredContent?: unknown;
}

export interface ChatToolDefinition {
  name: string;
  description: string;
  inputSchema: Record<string, unknown>;
  source: string;
  serverId?: string;
  sensitive: boolean;
}