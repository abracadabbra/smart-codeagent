import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type { ChatMcpServer, McpServerState } from "@/types/mcp";

interface McpStoreState {
  servers: ChatMcpServer[];
  states: Record<string, McpServerState>;
  loading: boolean;
  saving: boolean;
  showSettings: boolean;
  theme: "dark" | "light";

  setServers: (servers: ChatMcpServer[]) => void;
  setState: (serverId: string, state: McpServerState) => void;
  replaceStates: (states: Record<string, McpServerState>) => void;
  clear: () => void;
  setLoading: (loading: boolean) => void;
  setSaving: (saving: boolean) => void;
  setShowSettings: (show: boolean) => void;
  setTheme: (theme: "dark" | "light") => Promise<void>;

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
  theme: "dark",

  setServers: (servers) => set({ servers }),
  setState: (serverId, state) =>
    set((s) => ({ states: { ...s.states, [serverId]: state } })),
  replaceStates: (states) => set({ states }),
  clear: () => set({ servers: [], states: {} }),
  setLoading: (loading) => set({ loading }),
  setSaving: (saving) => set({ saving }),
  setShowSettings: (show) => set({ showSettings: show }),

  setTheme: async (theme) => {
    set({ theme });
    document.body.className = theme === "light" ? "light" : "";
    await saveSettingsToBackend(get().servers, theme);
    await get().reloadSettings();
  },

  addServer: async (server) => {
    const servers = [...get().servers, server];
    await saveSettingsToBackend(servers, get().theme);
    await get().reloadSettings();
  },

  updateServer: async (id, updates) => {
    const servers = get().servers.map((s) =>
      s.id === id ? { ...s, ...updates } : s,
    );
    await saveSettingsToBackend(servers, get().theme);
    await get().reloadSettings();
  },

  deleteServer: async (id) => {
    const servers = get().servers.filter((s) => s.id !== id);
    await saveSettingsToBackend(servers, get().theme);
    await get().reloadSettings();
  },

  toggleServerEnabled: async (id) => {
    const servers = get().servers.map((s) =>
      s.id === id ? { ...s, enabled: !s.enabled } : s,
    );
    await saveSettingsToBackend(servers, get().theme);
    await get().reloadSettings();
  },

  saveSettings: async () => {
    set({ saving: true });
    try {
      await saveSettingsToBackend(get().servers, get().theme);
    } finally {
      set({ saving: false });
    }
  },

  reloadSettings: async () => {
    set({ loading: true });
    try {
      const [servers, states, theme] = await Promise.all([
        invoke<ChatMcpServer[]>("list_mcp_servers"),
        invoke<Record<string, McpServerState>>("list_mcp_server_states"),
        invoke<string>("get_settings"),
      ]);
      set({ servers, states });
      const parsedTheme = (JSON.parse(theme) as { theme: string }).theme || "dark";
      set({ theme: parsedTheme as "dark" | "light" });
      document.body.className = parsedTheme === "light" ? "light" : "";
    } finally {
      set({ loading: false });
    }
  },

  testServer: async (server) => {
    await invoke("test_mcp_server", { server });
  },
}));

async function saveSettingsToBackend(servers: ChatMcpServer[], theme: string = "dark") {
  await invoke("save_settings", {
    settings: {
      mcp: {
        servers,
      },
      theme,
    },
  });
}

export async function initMcpStore() {
  try {
    const [servers, states, settingsJson] = await Promise.all([
      invoke<ChatMcpServer[]>("list_mcp_servers"),
      invoke<Record<string, McpServerState>>("list_mcp_server_states"),
      invoke<string>("get_settings"),
    ]);
    useMcpStore.setState({ servers, states });
    const parsed = JSON.parse(settingsJson) as { theme: string };
    const theme = (parsed.theme || "dark") as "dark" | "light";
    useMcpStore.setState({ theme });
    document.body.className = theme === "light" ? "light" : "";
  } catch (err) {
    // eslint-disable-next-line no-console
    console.warn("[mcpStore] init failed (non-Tauri env?):", err);
  }
}
