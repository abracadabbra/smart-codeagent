import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type { AgentState } from "@/types/agent";
import type {
  AgentApprovalRequestPayload,
  AgentAskUserPromptPayload,
} from "@/types/event";

interface AgentStoreState {
  // ---- Backward-compatible "current session" shortcuts ----
  state: AgentState;
  lastError: string | null;
  approvalRequest: AgentApprovalRequestPayload | null;
  askUserPrompt: AgentAskUserPromptPayload | null;

  // ---- Per-session buckets ----
  statesBySession: Record<string, AgentState>;
  lastErrorsBySession: Record<string, string | null>;
  approvalRequestsBySession: Record<string, AgentApprovalRequestPayload | null>;
  askUserPromptsBySession: Record<string, AgentAskUserPromptPayload | null>;
  activeConversationId: string | null;

  // ---- Backward-compatible actions ----
  setState: (state: AgentState) => void;
  setError: (message: string | null) => void;
  setApprovalRequest: (req: AgentApprovalRequestPayload | null) => void;
  clearApproval: () => void;
  setAskUserPrompt: (prompt: AgentAskUserPromptPayload | null) => void;
  clearAskUser: () => void;
  clearPrompts: () => void;
  sendCancel: () => Promise<void>;
  sendApproval: (allow: boolean) => Promise<void>;
  sendAskUserAnswer: (response: { phase: string; answers: Record<string, unknown> }) => Promise<void>;

  // ---- Per-session actions ----
  setStateFor: (conversationId: string, state: AgentState) => void;
  setErrorFor: (conversationId: string, message: string | null) => void;
  setApprovalRequestFor: (
    conversationId: string,
    req: AgentApprovalRequestPayload | null,
  ) => void;
  setAskUserPromptFor: (
    conversationId: string,
    prompt: AgentAskUserPromptPayload | null,
  ) => void;
  sendCancelFor: (conversationId: string) => Promise<void>;
  sendApprovalFor: (conversationId: string, allow: boolean) => Promise<void>;
  sendAskUserAnswerFor: (
    conversationId: string,
    response: { phase: string; answers: Record<string, unknown> },
  ) => Promise<void>;

  // Session switching
  setActiveConversation: (id: string | null) => void;
}

export const useAgentStore = create<AgentStoreState>((set, get) => ({
  state: "Idle",
  lastError: null,
  approvalRequest: null,
  askUserPrompt: null,

  statesBySession: {},
  lastErrorsBySession: {},
  approvalRequestsBySession: {},
  askUserPromptsBySession: {},
  activeConversationId: null,

  // ---- Backward-compatible shortcuts ----

  setState: (st) => {
    get().setStateFor(get().activeConversationId ?? "", st);
  },

  setError: (message) => {
    get().setErrorFor(get().activeConversationId ?? "", message);
  },

  setApprovalRequest: (req) => {
    get().setApprovalRequestFor(get().activeConversationId ?? "", req);
  },

  clearApproval: () => {
    get().setApprovalRequestFor(get().activeConversationId ?? "", null);
  },

  setAskUserPrompt: (prompt) => {
    get().setAskUserPromptFor(get().activeConversationId ?? "", prompt);
  },

  clearAskUser: () => {
    get().setAskUserPromptFor(get().activeConversationId ?? "", null);
  },

  clearPrompts: () => {
    const convId = get().activeConversationId ?? "";
    get().setApprovalRequestFor(convId, null);
    get().setAskUserPromptFor(convId, null);
  },

  sendCancel: () => {
    return get().sendCancelFor(get().activeConversationId ?? "");
  },

  sendApproval: (allow) => {
    return get().sendApprovalFor(get().activeConversationId ?? "", allow);
  },

  sendAskUserAnswer: (response) => {
    return get().sendAskUserAnswerFor(get().activeConversationId ?? "", response);
  },

  // ---- Per-session actions ----

  setStateFor: (conversationId, st) => {
    set((state) => {
      const patch: Partial<AgentStoreState> = {
        statesBySession: { ...state.statesBySession, [conversationId]: st },
        lastErrorsBySession: { ...state.lastErrorsBySession, [conversationId]: null },
      };
      if (state.activeConversationId === conversationId) {
        patch.state = st;
        patch.lastError = null;
      }
      return patch;
    });
  },

  setErrorFor: (conversationId, message) => {
    set((state) => {
      const patch: Partial<AgentStoreState> = {
        lastErrorsBySession: { ...state.lastErrorsBySession, [conversationId]: message },
      };
      if (state.activeConversationId === conversationId) {
        patch.lastError = message;
      }
      return patch;
    });
  },

  setApprovalRequestFor: (conversationId, req) => {
    set((state) => {
      const patch: Partial<AgentStoreState> = {
        approvalRequestsBySession: { ...state.approvalRequestsBySession, [conversationId]: req },
      };
      if (state.activeConversationId === conversationId) {
        patch.approvalRequest = req;
      }
      return patch;
    });
  },

  setAskUserPromptFor: (conversationId, prompt) => {
    set((state) => {
      const patch: Partial<AgentStoreState> = {
        askUserPromptsBySession: { ...state.askUserPromptsBySession, [conversationId]: prompt },
      };
      if (state.activeConversationId === conversationId) {
        patch.askUserPrompt = prompt;
      }
      return patch;
    });
  },

  sendCancelFor: async (conversationId) => {
    try {
      await invoke("cancel_run", { conversationId } as Record<string, unknown>);
    } catch (err) {
      // eslint-disable-next-line no-console
      console.error("[agentStore] cancel_run failed:", err);
    }
  },

  sendApprovalFor: async (conversationId, allow) => {
    const { approvalRequestsBySession } = get();
    const req = approvalRequestsBySession[conversationId];
    if (!req) return;
    try {
      await invoke("approve_tool", {
        args: {
          conversationId,
          approvalId: req.approvalId,
          allow,
        },
      } as Record<string, unknown>);
    } catch (err) {
      // eslint-disable-next-line no-console
      console.error("[agentStore] approve_tool failed:", err);
    }
  },

  sendAskUserAnswerFor: async (conversationId, response) => {
    const { askUserPromptsBySession } = get();
    const prompt = askUserPromptsBySession[conversationId];
    if (!prompt) return;
    try {
      await invoke("answer_ask_user", {
        args: {
          conversationId,
          askUserId: prompt.askUserId,
          response,
        },
      } as Record<string, unknown>);
    } catch (err) {
      // eslint-disable-next-line no-console
      console.error("[agentStore] answer_ask_user failed:", err);
    }
  },

  setActiveConversation: (id) => {
    set((state) => {
      if (state.activeConversationId === id) return {};
      const saved: Partial<AgentStoreState> = {
        activeConversationId: id,
      };
      if (state.activeConversationId != null) {
        saved.statesBySession = {
          ...state.statesBySession,
          [state.activeConversationId]: state.state,
        };
        saved.lastErrorsBySession = {
          ...state.lastErrorsBySession,
          [state.activeConversationId]: state.lastError,
        };
        saved.approvalRequestsBySession = {
          ...state.approvalRequestsBySession,
          [state.activeConversationId]: state.approvalRequest,
        };
        saved.askUserPromptsBySession = {
          ...state.askUserPromptsBySession,
          [state.activeConversationId]: state.askUserPrompt,
        };
      }
      if (id != null) {
        saved.state = state.statesBySession[id] ?? "Idle";
        saved.lastError = state.lastErrorsBySession[id] ?? null;
        saved.approvalRequest = state.approvalRequestsBySession[id] ?? null;
        saved.askUserPrompt = state.askUserPromptsBySession[id] ?? null;
      } else {
        saved.state = "Idle";
        saved.lastError = null;
        saved.approvalRequest = null;
        saved.askUserPrompt = null;
      }
      return saved;
    });
  },
}));
