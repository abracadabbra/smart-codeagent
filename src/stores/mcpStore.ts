// MCP server 状态 store。
//
// 启动时调 initMcpStore() 拉一次 list_mcp_servers + list_mcp_server_states；
// 之后由 useAgentEvents 订阅 mcp-server-state 事件增量更新。

import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type { ChatMcpServer, McpServerState } from "@/types/mcp";

interface McpStoreState {
  /** 配置的所有 MCP server（来自 settings.json）。 */
  servers: ChatMcpServer[];
  /** server_id -> 连接状态快照。未在 map 中的 server 视作 Disconnected。 */
  states: Record<string, McpServerState>;

  setServers: (servers: ChatMcpServer[]) => void;
  setState: (serverId: string, state: McpServerState) => void;
  replaceStates: (states: Record<string, McpServerState>) => void;
  clear: () => void;
}

export const useMcpStore = create<McpStoreState>((set) => ({
  servers: [],
  states: {},

  setServers: (servers) => set({ servers }),
  setState: (serverId, state) =>
    set((s) => ({ states: { ...s.states, [serverId]: state } })),
  replaceStates: (states) => set({ states }),
  clear: () => set({ servers: [], states: {} }),
}));

/**
 * 启动时拉取一次 server 列表 + 状态快照。
 * 在 Tauri 环境外（vite dev 单独跑）静默失败。
 */
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
