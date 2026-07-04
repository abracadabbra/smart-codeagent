import { create } from "zustand";
import type { AgentState } from "@/types/agent";

interface AgentStoreState {
  state: AgentState;
  lastError: string | null;

  setState: (state: AgentState) => void;
  setError: (message: string | null) => void;
}

export const useAgentStore = create<AgentStoreState>((set) => ({
  state: "Idle",
  lastError: null,

  setState: (state) => set({ state, lastError: null }),
  setError: (message) => set({ lastError: message }),
}));