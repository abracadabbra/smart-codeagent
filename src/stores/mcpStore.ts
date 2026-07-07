import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type { ChatMcpServer, McpServerState } from "@/types/mcp";

interface McpStoreState {
  servers: ChatMcpServer[];
  states: Record<string, McpServerState>;
  loading: boolean;
  saving: boolean;
  showSettings: boolean;

  setServers: (servers: ChatMcpServer[]) => void;
  setState: (serverId: string, state: McpServerState) => void;
  replaceStates: (states: Record<string, McpServerState>) => void;
  clear: () => void;
  setLoading: (loading: boolean) => void;
  setSaving: (saving: boolean) => void;
  setShowSettings: (show: boolean) => void;

  addServer: (server: ChatMcpServer) => Promise<void>;
  updateServer: (id: string, updates: Partial<ChatMcpServer>) => Promise<void>;
  deleteServer: (id: string) => Promise<void>;
  toggleServerEnabled: (id: string) => Promise<void>;
  saveSettings: () => Promise<void>;
  reloadSettings: () => Promise<void>;
  testServer: (server: ChatMcpServer) => Promise<void>;
}

export const useMcpStore = create<McpStoreState>((set, get) => ({
  servers: [],
  states: {},
  loading: false,
  saving: false,
  showSettings: false,

  setServers: (servers) => set({ servers }),
  setState: (serverId, state) =>
    set((s) => ({ states: { ...s.states, [serverId]: state } })),
  replaceStates: (states) => set({ states }),
  clear: () => set({ servers: [], states: {} }),
  setLoading: (loading) => set({ loading }),
  setSaving: (saving) => set({ saving }),
  setShowSettings: (show) => set({ showSettings: show }),

  addServer: async (server) => {
    const servers = [...get().servers, server];
    await saveSettingsToBackend(servers);
    await get().reloadSettings();
  },

  updateServer: async (id, updates) => {
    const servers = get().servers.map((s) =>
      s.id === id ? { ...s, ...updates } : s,
    );
    await saveSettingsToBackend(servers);
    await get().reloadSettings();
  },

  deleteServer: async (id) => {
    const servers = get().servers.filter((s) => s.id !== id);
    await saveSettingsToBackend(servers);
    await get().reloadSettings();
  },

  toggleServerEnabled: async (id) => {
    const servers = get().servers.map((s) =>
      s.id === id ? { ...s, enabled: !s.enabled } : s,
    );
    await saveSettingsToBackend(servers);
    await get().reloadSettings();
  },

  saveSettings: async () => {
    set({ saving: true });
    try {
      await saveSettingsToBackend(get().servers);
    } finally {
      set({ saving: false });
    }
  },

  reloadSettings: async () => {
    set({ loading: true });
    try {
      const [servers, states] = await Promise.all([
        invoke<ChatMcpServer[]>("list_mcp_servers"),
        invoke<Record<string, McpServerState>>("list_mcp_server_states"),
      ]);
      set({ servers, states });
    } finally {
      set({ loading: false });
    }
  },

  testServer: async (server) => {
    await invoke("test_mcp_server", { server });
  },
}));

async function saveSettingsToBackend(servers: ChatMcpServer[]) {
  await invoke("save_settings", {
    settings: {
      mcp: {
        servers,
      },
    },
  });
}

export async function initMcpStore() {
  try {
    const [servers, states] = await Promise.all([
      invoke<ChatMcpServer[]>("list_mcp_servers"),
      invoke<Record<string, McpServerState>>("list_mcp_server_states"),
    ]);
    useMcpStore.getState().setServers(servers);
    useMcpStore.getState().replaceStates(states);
  } catch (err) {
    // eslint-disable-next-line no-console
    console.warn("[mcpStore] init failed (non-Tauri env?):", err);
  }
}
