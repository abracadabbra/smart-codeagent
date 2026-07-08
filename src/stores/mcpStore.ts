import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type { ChatMcpServer, McpServerState } from "@/types/mcp";
import type { AppSettings, ProviderConfig } from "@/types/settings";
import { defaultProviderConfig } from "@/types/settings";

interface McpStoreState {
  servers: ChatMcpServer[];
  states: Record<string, McpServerState>;
  loading: boolean;
  saving: boolean;
  showSettings: boolean;
  theme: "dark" | "light";
  provider: ProviderConfig;

  setServers: (servers: ChatMcpServer[]) => void;
  setState: (serverId: string, state: McpServerState) => void;
  replaceStates: (states: Record<string, McpServerState>) => void;
  clear: () => void;
  setLoading: (loading: boolean) => void;
  setSaving: (saving: boolean) => void;
  setShowSettings: (show: boolean) => void;
  setTheme: (theme: "dark" | "light") => Promise<void>;
  setProvider: (provider: ProviderConfig) => Promise<void>;
  updateProvider: (updates: Partial<ProviderConfig>) => Promise<void>;

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
  provider: defaultProviderConfig(),

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
    await saveSettingsToBackend(get());
    await get().reloadSettings();
  },

  setProvider: async (provider) => {
    set({ provider });
    await saveSettingsToBackend(get());
    await get().reloadSettings();
  },

  updateProvider: async (updates) => {
    const provider = { ...get().provider, ...updates };
    set({ provider });
    await saveSettingsToBackend(get());
    await get().reloadSettings();
  },

  addServer: async (server) => {
    const servers = [...get().servers, server];
    set({ servers });
    await saveSettingsToBackend(get());
    await get().reloadSettings();
  },

  updateServer: async (id, updates) => {
    const servers = get().servers.map((s) =>
      s.id === id ? { ...s, ...updates } : s,
    );
    set({ servers });
    await saveSettingsToBackend(get());
    await get().reloadSettings();
  },

  deleteServer: async (id) => {
    const servers = get().servers.filter((s) => s.id !== id);
    set({ servers });
    await saveSettingsToBackend(get());
    await get().reloadSettings();
  },

  toggleServerEnabled: async (id) => {
    const servers = get().servers.map((s) =>
      s.id === id ? { ...s, enabled: !s.enabled } : s,
    );
    set({ servers });
    await saveSettingsToBackend(get());
    await get().reloadSettings();
  },

  saveSettings: async () => {
    set({ saving: true });
    try {
      await saveSettingsToBackend(get());
    } finally {
      set({ saving: false });
    }
  },

  reloadSettings: async () => {
    set({ loading: true });
    try {
      const [servers, states, settingsJson] = await Promise.all([
        invoke<ChatMcpServer[]>("list_mcp_servers"),
        invoke<Record<string, McpServerState>>("list_mcp_server_states"),
        invoke<string>("get_settings"),
      ]);
      set({ servers, states });
      const settings = JSON.parse(settingsJson) as AppSettings;
      const parsedTheme = settings.theme || "dark";
      set({ theme: parsedTheme as "dark" | "light" });
      set({ provider: settings.provider || defaultProviderConfig() });
      document.body.className = parsedTheme === "light" ? "light" : "";
    } finally {
      set({ loading: false });
    }
  },

  testServer: async (server) => {
    await invoke("test_mcp_server", { server });
  },
}));

async function saveSettingsToBackend(
  state: Pick<McpStoreState, "servers" | "theme" | "provider">,
) {
  await invoke("save_settings", {
    settings: {
      mcp: {
        servers: state.servers,
      },
      theme: state.theme,
      provider: state.provider,
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
    const settings = JSON.parse(settingsJson) as AppSettings;
    const theme = (settings.theme || "dark") as "dark" | "light";
    useMcpStore.setState({
      theme,
      provider: settings.provider || defaultProviderConfig(),
    });
    document.body.className = theme === "light" ? "light" : "";
  } catch (err) {
     
    console.warn("[mcpStore] init failed (non-Tauri env?):", err);
  }
}
