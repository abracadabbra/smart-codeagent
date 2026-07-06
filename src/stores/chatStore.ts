import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type { Message } from "@/types/message";
import type { ToolCallRecord } from "@/types/tool";

const newId = (prefix: string) =>
  `${prefix}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;

interface ChatState {
  // ---- Backward-compatible "current session" shortcuts (read by existing components) ----
  messages: Message[];
  currentAssistantId: string | null;
  toolRecordsByRun: Record<string, Record<string, ToolCallRecord>>;

  // ---- Per-session buckets (new architecture) ----
  messagesBySession: Record<string, Message[]>;
  currentAssistantIdBySession: Record<string, string | null>;
  toolRecordsBySession: Record<string, Record<string, Record<string, ToolCallRecord>>>;
  activeConversationId: string | null;

  // Lazy-loading pagination
  hasMoreBySession: Record<string, boolean>;
  oldestIndexBySession: Record<string, number>;

  // ---- Backward-compatible actions (operate on the "current" session) ----
  appendUserMessage: (text: string) => { userId: string; assistantId: string };
  prepareAssistantMessage: (id: string) => void;
  appendToken: (id: string, text: string) => void;
  appendStreamDelta: (id: string, text: string) => void;
  markComplete: (id: string) => void;
  markError: (id: string, error: string) => void;
  upsertToolRecord: (runId: string, record: ToolCallRecord) => void;
  clearRun: (runId: string) => void;
  setAssistantStatus: (status: Message["status"]) => void;
  resetAssistant: () => void;
  addToolRecord: (runId: string, toolCallId: string, record: ToolCallRecord) => void;
  updateToolRecord: (runId: string, toolCallId: string, record: ToolCallRecord) => void;
  setError: (message: string) => void;
  clear: () => void;

  // ---- Per-session actions (new architecture) ----
  appendUserMessageTo: (
    conversationId: string,
    text: string,
  ) => { userId: string; assistantId: string };
  prepareAssistantMessageFor: (conversationId: string, id: string) => void;
  appendTokenTo: (conversationId: string, id: string, text: string) => void;
  appendStreamDeltaTo: (conversationId: string, id: string, text: string) => void;
  markCompleteFor: (conversationId: string, id: string) => void;
  markErrorFor: (conversationId: string, id: string, error: string) => void;
  upsertToolRecordTo: (conversationId: string, runId: string, record: ToolCallRecord) => void;
  clearRunFor: (conversationId: string, runId: string) => void;
  setAssistantStatusFor: (conversationId: string, status: Message["status"]) => void;
  resetAssistantFor: (conversationId: string) => void;
  addToolRecordTo: (
    conversationId: string,
    runId: string,
    toolCallId: string,
    record: ToolCallRecord,
  ) => void;
  updateToolRecordTo: (
    conversationId: string,
    runId: string,
    toolCallId: string,
    record: ToolCallRecord,
  ) => void;
  setErrorFor: (conversationId: string, message: string) => void;
  clearSession: (conversationId: string) => void;

  // Session switching
  setActiveConversation: (id: string | null) => void;

  // Lazy loading
  loadMessagesPage: (conversationId: string) => Promise<boolean>;
}

function updateMessageById(
  messages: Message[],
  id: string,
  updater: (m: Message) => Message,
): Message[] {
  return messages.map((m) => (m.id === id ? updater(m) : m));
}

export const useChatStore = create<ChatState>((set, get) => ({
  messages: [],
  currentAssistantId: null,
  toolRecordsByRun: {},

  messagesBySession: {},
  currentAssistantIdBySession: {},
  toolRecordsBySession: {},
  activeConversationId: null,

  hasMoreBySession: {},
  oldestIndexBySession: {},

  // ---- Backward-compatible shortcuts ----

  appendUserMessage: (text) => {
    return get().appendUserMessageTo(get().activeConversationId ?? "", text);
  },

  prepareAssistantMessage: (id) => {
    get().prepareAssistantMessageFor(get().activeConversationId ?? "", id);
  },

  appendToken: (id, text) => {
    get().appendTokenTo(get().activeConversationId ?? "", id, text);
  },

  appendStreamDelta: (id, text) => {
    get().appendStreamDeltaTo(get().activeConversationId ?? "", id, text);
  },

  markComplete: (id) => {
    get().markCompleteFor(get().activeConversationId ?? "", id);
  },

  markError: (id, error) => {
    get().markErrorFor(get().activeConversationId ?? "", id, error);
  },

  upsertToolRecord: (runId, record) => {
    get().upsertToolRecordTo(get().activeConversationId ?? "", runId, record);
  },

  clearRun: (runId) => {
    get().clearRunFor(get().activeConversationId ?? "", runId);
  },

  setAssistantStatus: (status) => {
    get().setAssistantStatusFor(get().activeConversationId ?? "", status);
  },

  resetAssistant: () => {
    get().resetAssistantFor(get().activeConversationId ?? "");
  },

  addToolRecord: (runId, toolCallId, record) => {
    get().addToolRecordTo(get().activeConversationId ?? "", runId, toolCallId, record);
  },

  updateToolRecord: (runId, toolCallId, record) => {
    get().updateToolRecordTo(get().activeConversationId ?? "", runId, toolCallId, record);
  },

  setError: (message) => {
    get().setErrorFor(get().activeConversationId ?? "", message);
  },

  clear: () => {
    get().clearSession(get().activeConversationId ?? "");
  },

  // ---- Per-session actions ----

  appendUserMessageTo: (conversationId, text) => {
    const userId = newId("user");
    const assistantId = newId("asst");
    set((state) => {
      const existing = state.messagesBySession[conversationId] ?? [];
      const next = [
        ...existing,
        {
          id: userId,
          role: "user" as const,
          content: text,
          status: "complete" as const,
          createdAt: Date.now(),
        },
        {
          id: assistantId,
          role: "assistant" as const,
          content: "",
          status: "pending" as const,
          createdAt: Date.now(),
        },
      ];
      const patch: Partial<ChatState> = {
        messagesBySession: { ...state.messagesBySession, [conversationId]: next },
        currentAssistantIdBySession: {
          ...state.currentAssistantIdBySession,
          [conversationId]: assistantId,
        },
      };
      if (state.activeConversationId === conversationId) {
        patch.messages = next;
        patch.currentAssistantId = assistantId;
      }
      return patch;
    });
    return { userId, assistantId };
  },

  prepareAssistantMessageFor: (conversationId, id) => {
    set((state) => {
      const existing = state.messagesBySession[conversationId] ?? [];
      const next = updateMessageById(existing, id, (m) => ({ ...m, status: "streaming" }));
      const patch: Partial<ChatState> = {
        messagesBySession: { ...state.messagesBySession, [conversationId]: next },
      };
      if (state.activeConversationId === conversationId) {
        patch.messages = next;
      }
      return patch;
    });
  },

  appendTokenTo: (conversationId, id, text) => {
    set((state) => {
      const existing = state.messagesBySession[conversationId] ?? [];
      const next = updateMessageById(existing, id, (m) => ({
        ...m,
        content: m.content + text,
        status: "streaming",
      }));
      const patch: Partial<ChatState> = {
        messagesBySession: { ...state.messagesBySession, [conversationId]: next },
      };
      if (state.activeConversationId === conversationId) {
        patch.messages = next;
      }
      return patch;
    });
  },

  appendStreamDeltaTo: (conversationId, id, text) => {
    set((state) => {
      const existing = state.messagesBySession[conversationId] ?? [];
      const next = updateMessageById(existing, id, (m) => ({
        ...m,
        content: m.content + text,
        status: "streaming",
      }));
      const patch: Partial<ChatState> = {
        messagesBySession: { ...state.messagesBySession, [conversationId]: next },
      };
      if (state.activeConversationId === conversationId) {
        patch.messages = next;
      }
      return patch;
    });
  },

  markCompleteFor: (conversationId, id) => {
    set((state) => {
      const existing = state.messagesBySession[conversationId] ?? [];
      const next = updateMessageById(existing, id, (m) => ({ ...m, status: "complete" }));
      const patch: Partial<ChatState> = {
        messagesBySession: { ...state.messagesBySession, [conversationId]: next },
        currentAssistantIdBySession: {
          ...state.currentAssistantIdBySession,
          [conversationId]: null,
        },
      };
      if (state.activeConversationId === conversationId) {
        patch.messages = next;
        patch.currentAssistantId = null;
      }
      return patch;
    });
  },

  markErrorFor: (conversationId, id, error) => {
    set((state) => {
      const existing = state.messagesBySession[conversationId] ?? [];
      const next = updateMessageById(existing, id, (m) => ({
        ...m,
        status: "error",
        error,
        content: m.content || error,
      }));
      const patch: Partial<ChatState> = {
        messagesBySession: { ...state.messagesBySession, [conversationId]: next },
        currentAssistantIdBySession: {
          ...state.currentAssistantIdBySession,
          [conversationId]: null,
        },
      };
      if (state.activeConversationId === conversationId) {
        patch.messages = next;
        patch.currentAssistantId = null;
      }
      return patch;
    });
  },

  upsertToolRecordTo: (conversationId, runId, record) => {
    set((state) => {
      const sessionRecords = state.toolRecordsBySession[conversationId] ?? {};
      const next = {
        ...sessionRecords,
        [runId]: {
          ...(sessionRecords[runId] ?? {}),
          [record.id]: record,
        },
      };
      const patch: Partial<ChatState> = {
        toolRecordsBySession: { ...state.toolRecordsBySession, [conversationId]: next },
      };
      if (state.activeConversationId === conversationId) {
        patch.toolRecordsByRun = next;
      }
      return patch;
    });
  },

  clearRunFor: (conversationId, runId) => {
    set((state) => {
      const sessionRecords = state.toolRecordsBySession[conversationId] ?? {};
      const next = { ...sessionRecords };
      delete next[runId];
      const patch: Partial<ChatState> = {
        toolRecordsBySession: { ...state.toolRecordsBySession, [conversationId]: next },
      };
      if (state.activeConversationId === conversationId) {
        patch.toolRecordsByRun = next;
      }
      return patch;
    });
  },

  setAssistantStatusFor: (conversationId, status) => {
    set((state) => {
      const existing = state.messagesBySession[conversationId] ?? [];
      if (existing.length === 0) return {};
      const next = [...existing];
      const last = next[next.length - 1];
      next[next.length - 1] = { ...last, status };
      const patch: Partial<ChatState> = {
        messagesBySession: { ...state.messagesBySession, [conversationId]: next },
      };
      if (state.activeConversationId === conversationId) {
        patch.messages = next;
      }
      return patch;
    });
  },

  resetAssistantFor: (conversationId) => {
    set((state) => {
      const existing = state.messagesBySession[conversationId] ?? [];
      if (existing.length === 0) return {};
      const next = existing.slice(0, -1);
      const patch: Partial<ChatState> = {
        messagesBySession: { ...state.messagesBySession, [conversationId]: next },
        currentAssistantIdBySession: {
          ...state.currentAssistantIdBySession,
          [conversationId]: null,
        },
      };
      if (state.activeConversationId === conversationId) {
        patch.messages = next;
        patch.currentAssistantId = null;
      }
      return patch;
    });
  },

  addToolRecordTo: (conversationId, runId, toolCallId, record) => {
    set((state) => {
      const sessionRecords = state.toolRecordsBySession[conversationId] ?? {};
      const runRecords = sessionRecords[runId] ?? {};
      const next = {
        ...sessionRecords,
        [runId]: { ...runRecords, [toolCallId]: record },
      };
      const patch: Partial<ChatState> = {
        toolRecordsBySession: { ...state.toolRecordsBySession, [conversationId]: next },
      };
      if (state.activeConversationId === conversationId) {
        patch.toolRecordsByRun = next;
      }
      return patch;
    });
  },

  updateToolRecordTo: (conversationId, runId, toolCallId, record) => {
    set((state) => {
      const sessionRecords = state.toolRecordsBySession[conversationId] ?? {};
      const runRecords = sessionRecords[runId] ?? {};
      const next = {
        ...sessionRecords,
        [runId]: { ...runRecords, [toolCallId]: record },
      };
      const patch: Partial<ChatState> = {
        toolRecordsBySession: { ...state.toolRecordsBySession, [conversationId]: next },
      };
      if (state.activeConversationId === conversationId) {
        patch.toolRecordsByRun = next;
      }
      return patch;
    });
  },

  setErrorFor: (conversationId, message) => {
    set((state) => {
      const existing = state.messagesBySession[conversationId] ?? [];
      if (existing.length === 0) return {};
      const next = [...existing];
      const last = next[next.length - 1];
      next[next.length - 1] = {
        ...last,
        content: last.content || message,
        status: "error",
        error: message,
      };
      const patch: Partial<ChatState> = {
        messagesBySession: { ...state.messagesBySession, [conversationId]: next },
      };
      if (state.activeConversationId === conversationId) {
        patch.messages = next;
      }
      return patch;
    });
  },

  clearSession: (conversationId) => {
    set((state) => {
      const nextMessages = { ...state.messagesBySession };
      const nextAssistantIds = { ...state.currentAssistantIdBySession };
      const nextRecords = { ...state.toolRecordsBySession };
      delete nextMessages[conversationId];
      delete nextAssistantIds[conversationId];
      delete nextRecords[conversationId];
      const patch: Partial<ChatState> = {
        messagesBySession: nextMessages,
        currentAssistantIdBySession: nextAssistantIds,
        toolRecordsBySession: nextRecords,
      };
      if (state.activeConversationId === conversationId) {
        patch.messages = [];
        patch.currentAssistantId = null;
        patch.toolRecordsByRun = {};
      }
      return patch;
    });
  },

  setActiveConversation: (id) => {
    set((state) => {
      if (state.activeConversationId === id) return {};
      const saved: Partial<ChatState> = {
        activeConversationId: id,
      };
      if (state.activeConversationId != null) {
        saved.messagesBySession = {
          ...state.messagesBySession,
          [state.activeConversationId]: state.messages,
        };
        saved.currentAssistantIdBySession = {
          ...state.currentAssistantIdBySession,
          [state.activeConversationId]: state.currentAssistantId,
        };
        saved.toolRecordsBySession = {
          ...state.toolRecordsBySession,
          [state.activeConversationId]: state.toolRecordsByRun,
        };
      }
      if (id != null) {
        saved.messages = state.messagesBySession[id] ?? [];
        saved.currentAssistantId = state.currentAssistantIdBySession[id] ?? null;
        saved.toolRecordsByRun = state.toolRecordsBySession[id] ?? {};
      } else {
        saved.messages = [];
        saved.currentAssistantId = null;
        saved.toolRecordsByRun = {};
      }
      return saved;
    });
  },

  loadMessagesPage: async (conversationId) => {
    const { oldestIndexBySession, hasMoreBySession } = get();
    const oldest = oldestIndexBySession[conversationId] ?? Number.MAX_SAFE_INTEGER;
    const hasMore = hasMoreBySession[conversationId] ?? true;
    if (!hasMore) return false;

    try {
      const page = await invoke<{
        messages: Message[];
        total: number;
        hasMore: boolean;
      }>("get_session_messages", {
        conversationId,
        limit: 50,
        before: oldest < Number.MAX_SAFE_INTEGER ? oldest : null,
      });

      set((state) => {
        const existing = state.messagesBySession[conversationId] ?? [];
        const next = [...page.messages, ...existing];
        const patch: Partial<ChatState> = {
          messagesBySession: { ...state.messagesBySession, [conversationId]: next },
          hasMoreBySession: { ...state.hasMoreBySession, [conversationId]: page.hasMore },
          oldestIndexBySession: {
            ...state.oldestIndexBySession,
            [conversationId]: next.length > 0 ? next[0].createdAt : 0,
          },
        };
        if (state.activeConversationId === conversationId) {
          patch.messages = next;
        }
        return patch;
      });
      return page.hasMore;
    } catch (err) {
      // eslint-disable-next-line no-console
      console.error("[chatStore] loadMessagesPage failed:", err);
      return false;
    }
  },
}));
