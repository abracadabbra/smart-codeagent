import { create } from "zustand";
import type { AgentState } from "@/types/agent";
import type {
  AgentApprovalRequestPayload,
  AgentAskUserPromptPayload,
} from "@/types/event";

interface AgentStoreState {
  state: AgentState;
  lastError: string | null;
  /// Latest approval request (frontend shows modal); cleared after user responds.
  approvalRequest: AgentApprovalRequestPayload | null;
  /// Latest ask_user prompt (frontend shows AskUserPromptCard); cleared after answer.
  askUserPrompt: AgentAskUserPromptPayload | null;

  setState: (state: AgentState) => void;
  setError: (message: string | null) => void;
  setApprovalRequest: (req: AgentApprovalRequestPayload) => void;
  clearApproval: () => void;
  setAskUserPrompt: (req: AgentAskUserPromptPayload) => void;
  clearAskUser: () => void;
  clearPrompts: () => void;
}

export const useAgentStore = create<AgentStoreState>((set) => ({
  state: "Idle",
  lastError: null,
  approvalRequest: null,
  askUserPrompt: null,

  setState: (state) => set({ state, lastError: null }),
  setError: (message) => set({ lastError: message }),
  setApprovalRequest: (req) => set({ approvalRequest: req }),
  clearApproval: () => set({ approvalRequest: null }),
  setAskUserPrompt: (req) => set({ askUserPrompt: req }),
  clearAskUser: () => set({ askUserPrompt: null }),
  clearPrompts: () =>
    set({ approvalRequest: null, askUserPrompt: null }),
}));