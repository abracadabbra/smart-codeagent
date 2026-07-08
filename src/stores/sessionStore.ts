import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type { Conversation, ConversationListItem } from "@/types/session";
import type { AgentState } from "@/types/agent";
import { useChatStore } from "./chatStore";
import { useAgentStore } from "./agentStore";

interface SessionState {
  sessions: ConversationListItem[];
  activeSessionId: string | null;
  generatingIds: Set<string>;
  pendingApprovalIds: Set<string>;
  searchQuery: string;

  loadSessions: () => Promise<void>;
  selectSession: (id: string | null) => void;
  createSession: () => Promise<string>;
  renameSession: (id: string, title: string) => Promise<void>;
  deleteSession: (id: string) => Promise<void>;
  togglePin: (id: string) => Promise<void>;
  setSearchQuery: (q: string) => void;
  markGenerating: (id: string, generating: boolean) => void;
  addPendingApproval: (id: string) => void;
  removePendingApproval: (id: string) => void;
  forceResetSession: (id: string) => Promise<void>;
}

export const useSessionStore = create<SessionState>((set, get) => ({
  sessions: [],
  activeSessionId: null,
  generatingIds: new Set(),
  pendingApprovalIds: new Set(),
  searchQuery: "",

  loadSessions: async () => {
    try {
      const items = await invoke<ConversationListItem[]>("list_sessions");
      const { activeSessionId } = get();
      const nextActive = activeSessionId ?? (items.length > 0 ? items[0].id : null);
      set({ sessions: items, activeSessionId: nextActive });

      // 全局同步：一次性拉取所有非 Idle 会话，同步 agentStore + generatingIds
      // 这样能立刻发现"卡住"的会话（后端 Running 但前端不知道）
      try {
        const active = await invoke<[string, string][]>("list_active_sessions");
        const activeIds = new Set<string>();
        for (const [convId, stateStr] of active) {
          activeIds.add(convId);
          useAgentStore.getState().setStateFor(convId, stateStr as AgentState);
        }
        // 所有非 Idle 的会话标记为 generating
        set({ generatingIds: new Set(activeIds) });
      } catch {
        // 忽略：命令不可用时降级
      }

      if (nextActive) {
        useChatStore.getState().setActiveConversation(nextActive);
        useAgentStore.getState().setActiveConversation(nextActive);
        // 加载首个会话的历史消息
        const chat = useChatStore.getState();
        const cached = chat.messagesBySession[nextActive];
        if (!cached || cached.length === 0) {
          void chat.loadMessagesPage(nextActive);
        }
      }
    } catch (err) {
       
      console.error("[sessionStore] list_sessions failed:", err);
    }
  },

  selectSession: (id) => {
    set({ activeSessionId: id });
    useChatStore.getState().setActiveConversation(id);
    useAgentStore.getState().setActiveConversation(id);
    // 切换会话时加载历史消息（仅在内存缓存为空时加载）
    if (id) {
      const chat = useChatStore.getState();
      const cached = chat.messagesBySession[id];
      if (!cached || cached.length === 0) {
        void chat.loadMessagesPage(id);
      }
      // 安全网：切换会话时同步真实状态
      void (async () => {
        try {
          const realState = await invoke<string>("get_session_state", {
            conversationId: id,
          });
          useAgentStore.getState().setStateFor(id, realState as AgentState);
          if (realState === "Idle") {
            get().markGenerating(id, false);
          }
        } catch {
          // 忽略
        }
      })();
    }
  },

  createSession: async () => {
    try {
      const conv = await invoke<Conversation>("create_session");
      const { sessions } = get();
      const item: ConversationListItem = {
        id: conv.id,
        title: conv.title,
        preview: "",
        createdAt: conv.createdAt,
        updatedAt: conv.updatedAt,
        pinned: conv.pinned,
        messageCount: conv.messageCount,
      };
      const next = [...sessions, item];
      // sort: pinned desc, updatedAt desc
      next.sort((a, b) => {
        if (a.pinned !== b.pinned) return (b.pinned ? 1 : 0) - (a.pinned ? 1 : 0);
        return b.updatedAt - a.updatedAt;
      });
      set({ sessions: next, activeSessionId: conv.id });
      useChatStore.getState().setActiveConversation(conv.id);
      useAgentStore.getState().setActiveConversation(conv.id);
      return conv.id;
    } catch (err) {
       
      console.error("[sessionStore] create_session failed:", err);
      throw err;
    }
  },

  renameSession: async (id, title) => {
    try {
      const updated = await invoke<Conversation>("update_session", {
        conversationId: id,
        title,
      });
      set((state) => ({
        sessions: state.sessions.map((s) =>
          s.id === id ? { ...s, title: updated.title, updatedAt: updated.updatedAt } : s,
        ),
      }));
    } catch (err) {
       
      console.error("[sessionStore] update_session failed:", err);
    }
  },

  deleteSession: async (id) => {
    try {
      await invoke("delete_session", { conversationId: id });
      set((state) => {
        const next = state.sessions.filter((s) => s.id !== id);
        const nextActive =
          state.activeSessionId === id
            ? next.length > 0
              ? next[0].id
              : null
            : state.activeSessionId;
        if (nextActive !== state.activeSessionId) {
          useChatStore.getState().setActiveConversation(nextActive);
          useAgentStore.getState().setActiveConversation(nextActive);
        }
        return { sessions: next, activeSessionId: nextActive };
      });
    } catch (err) {
       
      console.error("[sessionStore] delete_session failed:", err);
    }
  },

  togglePin: async (id) => {
    const { sessions } = get();
    const item = sessions.find((s) => s.id === id);
    if (!item) return;
    try {
      const updated = await invoke<Conversation>("update_session", {
        conversationId: id,
        pinned: !item.pinned,
      });
      set((state) => {
        const next = state.sessions.map((s) =>
          s.id === id
            ? { ...s, pinned: updated.pinned, updatedAt: updated.updatedAt }
            : s,
        );
        next.sort((a, b) => {
          if (a.pinned !== b.pinned)
            return (b.pinned ? 1 : 0) - (a.pinned ? 1 : 0);
          return b.updatedAt - a.updatedAt;
        });
        return { sessions: next };
      });
    } catch (err) {
       
      console.error("[sessionStore] update_session (pin) failed:", err);
    }
  },

  setSearchQuery: (q) => set({ searchQuery: q }),

  markGenerating: (id, generating) => {
    set((state) => {
      const next = new Set(state.generatingIds);
      if (generating) next.add(id);
      else next.delete(id);
      return { generatingIds: next };
    });
  },

  addPendingApproval: (id) => {
    set((state) => {
      const next = new Set(state.pendingApprovalIds);
      next.add(id);
      return { pendingApprovalIds: next };
    });
  },

  removePendingApproval: (id) => {
    set((state) => {
      const next = new Set(state.pendingApprovalIds);
      next.delete(id);
      return { pendingApprovalIds: next };
    });
  },

  forceResetSession: async (id) => {
    try {
      await invoke("force_reset_session", { conversationId: id });
      useAgentStore.getState().setStateFor(id, "Idle");
      get().markGenerating(id, false);
    } catch (err) {
       
      console.error("[sessionStore] force_reset_session failed:", err);
    }
  },
}));

export function initSessionStore() {
  void useSessionStore.getState().loadSessions();
}
