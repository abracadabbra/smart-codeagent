// MCP server 状态栏 — 显示在主区域底部。
//
// - 无 enabled server 时显示 "No MCP servers"
// - 有 server 时显示 "N connected / M error / total" 概要
// - hover 展开完整 server 列表（name + state + error.message）

import { useMcpStore } from "@/stores/mcpStore";
import type { McpServerState } from "@/types/mcp";

function stateLabel(state: McpServerState | undefined): string {
  switch (state?.kind) {
    case "connected":
      return "Connected";
    case "connecting":
      return "Connecting…";
    case "error":
      return `Error: ${state.message}`;
    case "disconnected":
    default:
      return "Disconnected";
  }
}

function stateColor(state: McpServerState | undefined): string {
  switch (state?.kind) {
    case "connected":
      return "text-green-400";
    case "connecting":
      return "text-yellow-400";
    case "error":
      return "text-red-400";
    case "disconnected":
    default:
      return "text-ink-500";
  }
}

function stateDot(state: McpServerState | undefined): string {
  switch (state?.kind) {
    case "connected":
      return "bg-green-400";
    case "connecting":
      return "bg-yellow-400";
    case "error":
      return "bg-red-400";
    case "disconnected":
    default:
      return "bg-ink-600";
  }
}

export function StatusBar() {
  const servers = useMcpStore((s) => s.servers);
  const states = useMcpStore((s) => s.states);

  const enabledServers = servers.filter((s) => s.enabled);
  const connected = enabledServers.filter(
    (s) => states[s.id]?.kind === "connected",
  ).length;
  const errored = enabledServers.filter(
    (s) => states[s.id]?.kind === "error",
  ).length;

  return (
    <footer className="h-7 shrink-0 border-t border-ink-800/60 bg-ink-900/50 flex items-center px-3 text-[11px] text-ink-400 gap-2 group relative">
      <span className="font-semibold text-ink-300">MCP</span>
      {enabledServers.length === 0 ? (
        <span className="text-ink-500">No MCP servers</span>
      ) : (
        <>
          <span className="text-green-400">{connected} connected</span>
          {errored > 0 && (
            <span className="text-red-400">{errored} error</span>
          )}
          <span className="text-ink-500">/ {enabledServers.length}</span>
        </>
      )}

      {/* hover 展开列表 */}
      {enabledServers.length > 0 && (
        <div className="hidden group-hover:block absolute bottom-full left-2 mb-1 bg-ink-900 border border-ink-700 rounded-lg shadow-xl p-1.5 min-w-[260px] max-w-[400px] z-50">
          {enabledServers.map((s) => {
            const state = states[s.id];
            return (
              <div
                key={s.id}
                className="flex items-center justify-between py-1 px-2 text-xs gap-2"
              >
                <div className="flex items-center gap-2 min-w-0">
                  <span
                    className={`inline-block w-1.5 h-1.5 rounded-full shrink-0 ${stateDot(state)}`}
                  />
                  <span className="text-ink-200 truncate">{s.name}</span>
                </div>
                <span className={`shrink-0 ${stateColor(state)}`}>
                  {stateLabel(state)}
                </span>
              </div>
            );
          })}
        </div>
      )}
    </footer>
  );
}
