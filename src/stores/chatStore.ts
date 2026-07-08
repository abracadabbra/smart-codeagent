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

  // UI: 右侧面板开关
  previewOpen: boolean;
  togglePreview: () => void;
  setPreviewOpen: (open: boolean) => void;

  // UI: 消息搜索
  searchOpen: boolean;
  searchQuery: string;
  searchResults: number[];
  searchCurrentIndex: number;
  toggleSearch: () => void;
  setSearchOpen: (open: boolean) => void;
  setSearchQuery: (query: string) => void;
  nextSearchResult: () => void;
  prevSearchResult: () => void;
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

  previewOpen: true,

  searchOpen: false,
  searchQuery: "",
  searchResults: [],
  searchCurrentIndex: -1,

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
      next[next.length - 1] = { ...last, status: "error", error: message };
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
      const nextRecords = { ...state.toolRecordsBySession };
      const nextAssistantIds = { ...state.currentAssistantIdBySession };
      const nextHasMore = { ...state.hasMoreBySession };
      const nextOldest = { ...state.oldestIndexBySession };
      delete nextMessages[conversationId];
      delete nextRecords[conversationId];
      delete nextAssistantIds[conversationId];
      delete nextHasMore[conversationId];
      delete nextOldest[conversationId];
      const patch: Partial<ChatState> = {
        messagesBySession: nextMessages,
        toolRecordsBySession: nextRecords,
        currentAssistantIdBySession: nextAssistantIds,
        hasMoreBySession: nextHasMore,
        oldestIndexBySession: nextOldest,
      };
      if (state.activeConversationId === conversationId) {
        patch.messages = [];
        patch.toolRecordsByRun = {};
        patch.currentAssistantId = null;
      }
      return patch;
    });
  },

  // Session switching

  setActiveConversation: (id) => {
    set((state) => {
      const patch: Partial<ChatState> = { activeConversationId: id };
      if (id) {
        patch.messages = state.messagesBySession[id] ?? [];
        patch.toolRecordsByRun = state.toolRecordsBySession[id] ?? {};
        patch.currentAssistantId = state.currentAssistantIdBySession[id] ?? null;
      } else {
        patch.messages = [];
        patch.toolRecordsByRun = {};
        patch.currentAssistantId = null;
      }
      return patch;
    });
  },

  // Lazy loading

  loadMessagesPage: async (conversationId) => {
    const state = get();
    const oldest = state.oldestIndexBySession[conversationId] ?? Number.MAX_SAFE_INTEGER;
    try {
      // 后端命令是 get_session_messages（返回 SessionMessagesPage: {messages, total, hasMore}）
      const page = await invoke<{ messages: Message[]; total: number; hasMore: boolean }>(
        "get_session_messages",
        {
          conversationId,
          before: oldest === Number.MAX_SAFE_INTEGER ? null : oldest,
          limit: 50,
        },
      );
      const msgs = page?.messages ?? [];
      if (msgs.length === 0) {
        set((s) => ({
          hasMoreBySession: { ...s.hasMoreBySession, [conversationId]: false },
        }));
        return false;
      }
      set((s) => {
        const existing = s.messagesBySession[conversationId] ?? [];
        const nextMap = new Map<string, Message>();
        msgs.forEach((m) => nextMap.set(m.id, m));
        existing.forEach((m) => {
          if (!nextMap.has(m.id)) nextMap.set(m.id, m);
        });
        const next = Array.from(nextMap.values()).sort((a, b) => a.createdAt - b.createdAt);
        const patch: Partial<ChatState> = {
          messagesBySession: { ...s.messagesBySession, [conversationId]: next },
          oldestIndexBySession: {
            ...s.oldestIndexBySession,
            [conversationId]: Math.min(...msgs.map((m) => m.createdAt), oldest),
          },
          hasMoreBySession: {
            ...s.hasMoreBySession,
            [conversationId]: page.hasMore,
          },
        };
        if (s.activeConversationId === conversationId) {
          patch.messages = next;
        }
        return patch;
      });
      return true;
    } catch (err) {
       
      console.error("[chatStore] loadMessagesPage failed:", err);
      set((s) => ({
        hasMoreBySession: { ...s.hasMoreBySession, [conversationId]: false },
      }));
      return false;
    }
  },

  // UI: 右侧面板开关

  togglePreview: () => {
    set((state) => ({ previewOpen: !state.previewOpen }));
  },

  setPreviewOpen: (open) => {
    set({ previewOpen: open });
  },

  // UI: 消息搜索

  toggleSearch: () => {
    set((state) => {
      const nextOpen = !state.searchOpen;
      if (!nextOpen) {
        return {
          searchOpen: false,
          searchQuery: "",
          searchResults: [],
          searchCurrentIndex: -1,
        };
      }
      const { results } = computeSearchResults(state.messages, state.searchQuery);
      return {
        searchOpen: true,
        searchResults: results,
        searchCurrentIndex: results.length > 0 ? 0 : -1,
      };
    });
  },

  setSearchOpen: (open) => {
    set((state) => {
      if (!open) {
        return {
          searchOpen: false,
          searchQuery: "",
          searchResults: [],
          searchCurrentIndex: -1,
        };
      }
      const { results } = computeSearchResults(state.messages, state.searchQuery);
      return {
        searchOpen: true,
        searchResults: results,
        searchCurrentIndex: results.length > 0 ? 0 : -1,
      };
    });
  },

  setSearchQuery: (query) => {
    set((state) => {
      const { results } = computeSearchResults(state.messages, query);
      const currentIndex = results.length > 0 ? 0 : -1;
      return {
        searchQuery: query,
        searchResults: results,
        searchCurrentIndex: currentIndex,
      };
    });
  },

  nextSearchResult: () => {
    set((state) => {
      if (state.searchResults.length === 0) return {};
      const nextIndex =
        state.searchCurrentIndex + 1 >= state.searchResults.length
          ? 0
          : state.searchCurrentIndex + 1;
      return { searchCurrentIndex: nextIndex };
    });
  },

  prevSearchResult: () => {
    set((state) => {
      if (state.searchResults.length === 0) return {};
      const prevIndex =
        state.searchCurrentIndex - 1 < 0
          ? state.searchResults.length - 1
          : state.searchCurrentIndex - 1;
      return { searchCurrentIndex: prevIndex };
    });
  },
}));

function computeSearchResults(messages: Message[], query: string): { results: number[] } {
  if (!query.trim()) return { results: [] };
  const lower = query.toLowerCase();
  const results = messages
    .map((m, i) => ({ i, match: (m.content ?? "").toLowerCase().includes(lower) }))
    .filter((x) => x.match)
    .map((x) => x.i);
  return { results };
}
