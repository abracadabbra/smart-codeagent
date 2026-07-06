import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type { Conversation, ConversationListItem } from "@/types/session";

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
      set({ sessions: items });
    } catch (err) {
      // eslint-disable-next-line no-console
      console.error("[sessionStore] list_sessions failed:", err);
    }
  },

  selectSession: (id) => {
    set({ activeSessionId: id });
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
      return conv.id;
    } catch (err) {
      // eslint-disable-next-line no-console
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
      // eslint-disable-next-line no-console
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
        return { sessions: next, activeSessionId: nextActive };
      });
    } catch (err) {
      // eslint-disable-next-line no-console
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
      // eslint-disable-next-line no-console
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
}));

export function initSessionStore() {
  void useSessionStore.getState().loadSessions();
}
