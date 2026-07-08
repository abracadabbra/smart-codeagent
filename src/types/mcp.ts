// MCP 相关 TypeScript 类型 — 与 src-tauri/src/mcp/types.rs + settings.rs 保持同步。
//
// Rust 端：
// - `McpServerState` 是 `#[serde(tag = "kind", rename_all = "lowercase")]` 枚举
// - `McpServerStatePayload` / `ChatMcpServer` 是 `#[serde(rename_all = "camelCase")]`
//
// 前端 wire 格式示例：
//   state = { "kind": "connected" } | { "kind": "error", "message": "..." }
//   payload = { "serverId": "fs", "state": { "kind": "connected" } }

/** MCP server 连接状态（tagged union，对应 Rust McpServerState 枚举）。 */
export type McpServerState =
  | { kind: "connecting" }
  | { kind: "connected" }
  | { kind: "error"; message: string }
  | { kind: "disconnected" };

/** `mcp-server-state` 事件 payload。 */
export interface McpServerStatePayload {
  serverId: string;
  state: McpServerState;
}

/** settings.json 中的 MCP server 配置（对应 Rust ChatMcpServer）。 */
export interface ChatMcpServer {
  id: string;
  name: string;
  enabled: boolean;
  transport: string;
  command: string;
  args: string[];
  env: Record<string, string>;
  cwd?: string | null;
  url?: string | null;
  headers?: Record<string, string> | null;
  enabledTools: string[];
}

/** 事件名常量 */
export const EVT_MCP_SERVER_STATE = "mcp-server-state";
