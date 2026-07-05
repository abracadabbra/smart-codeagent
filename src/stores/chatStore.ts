import { create } from "zustand";
import type { Message } from "@/types/message";
import type { ToolCallRecord } from "@/types/tool";

interface ChatState {
  messages: Message[];
  currentAssistantId: string | null;
  /// Per-runId tool records. Each tool call may update multiple times
  /// (Pending → Success / Cancelled / Error), so we keep them keyed by
  /// record id and upsert on each event.
  toolRecordsByRun: Record<string, Record<string, ToolCallRecord>>;

  appendUserMessage: (
    text: string,
  ) => { userId: string; assistantId: string };
  prepareAssistantMessage: (id: string) => void;
  appendToken: (id: string, text: string) => void;
  appendStreamDelta: (id: string, text: string) => void;
  markComplete: (id: string) => void;
  markError: (id: string, error: string) => void;
  upsertToolRecord: (runId: string, record: ToolCallRecord) => void;
  clearRun: (runId: string) => void;
}

const newId = (prefix: string) =>
  `${prefix}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;

export const useChatStore = create<ChatState>((set) => ({
  messages: [],
  currentAssistantId: null,
  toolRecordsByRun: {},

  appendUserMessage: (text) => {
    const userId = newId("user");
    const assistantId = newId("asst");

    set((state) => ({
      messages: [
        ...state.messages,
        {
          id: userId,
          role: "user",
          content: text,
          status: "complete",
          createdAt: Date.now(),
        },
        {
          id: assistantId,
          role: "assistant",
          content: "",
          status: "pending",
          createdAt: Date.now(),
        },
      ],
      currentAssistantId: assistantId,
    }));

    return { userId, assistantId };
  },

  prepareAssistantMessage: (id) => {
    set((state) => ({
      messages: state.messages.map((m) =>
        m.id === id ? { ...m, status: "streaming" } : m,
      ),
    }));
  },

  appendToken: (id, text) => {
    set((state) => ({
      messages: state.messages.map((m) =>
        m.id === id
          ? { ...m, content: m.content + text, status: "streaming" }
          : m,
      ),
    }));
  },

  appendStreamDelta: (id, text) => {
    set((state) => ({
      messages: state.messages.map((m) =>
        m.id === id
          ? { ...m, content: m.content + text, status: "streaming" }
          : m,
      ),
    }));
  },

  markComplete: (id) => {
    set((state) => ({
      messages: state.messages.map((m) =>
        m.id === id ? { ...m, status: "complete" } : m,
      ),
      currentAssistantId: null,
    }));
  },

  markError: (id, error) => {
    set((state) => ({
      messages: state.messages.map((m) =>
        m.id === id
          ? { ...m, status: "error", error, content: m.content || error }
          : m,
      ),
      currentAssistantId: null,
    }));
  },

  upsertToolRecord: (runId, record) => {
    set((state) => ({
      toolRecordsByRun: {
        ...state.toolRecordsByRun,
        [runId]: {
          ...(state.toolRecordsByRun[runId] ?? {}),
          [record.id]: record,
        },
      },
    }));
  },

  clearRun: (runId) => {
    set((state) => {
      const next = { ...state.toolRecordsByRun };
      delete next[runId];
      return { toolRecordsByRun: next };
    });
  },
}));