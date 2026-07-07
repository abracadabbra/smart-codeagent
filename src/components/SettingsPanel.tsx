import { useState } from "react";
import { useMcpStore } from "@/stores/mcpStore";
import { McpServerForm } from "./McpServerForm";
import type { ChatMcpServer, McpServerState } from "@/types/mcp";

interface SettingsPanelProps {
  onClose: () => void;
}

type Tab = "mcp" | "llm" | "app";

function getStatusIcon(state: McpServerState | undefined) {
  if (!state) {
    return { icon: "circle-dot", color: "text-ink-600", label: "未连接" };
  }
  switch (state.kind) {
    case "connecting":
      return { icon: "loader-2", color: "text-blue-400 animate-spin", label: "连接中..." };
    case "connected":
      return { icon: "check-circle-2", color: "text-green-400", label: "已连接" };
    case "error":
      return { icon: "alert-circle", color: "text-red-400", label: "错误" };
    case "disconnected":
      return { icon: "circle-dot", color: "text-ink-600", label: "已断开" };
  }
}

export function SettingsPanel({ onClose }: SettingsPanelProps) {
  const [activeTab, setActiveTab] = useState<Tab>("mcp");
  const [editingServer, setEditingServer] = useState<ChatMcpServer | null>(null);
  const [showAddForm, setShowAddForm] = useState(false);
  const [testResult, setTestResult] = useState<Record<string, "success" | "error" | null>>({});

  const servers = useMcpStore((s) => s.servers);
  const states = useMcpStore((s) => s.states);
  const loading = useMcpStore((s) => s.loading);
  const saving = useMcpStore((s) => s.saving);
  const theme = useMcpStore((s) => s.theme);

  const addServer = useMcpStore((s) => s.addServer);
  const updateServer = useMcpStore((s) => s.updateServer);
  const deleteServer = useMcpStore((s) => s.deleteServer);
  const toggleServerEnabled = useMcpStore((s) => s.toggleServerEnabled);
  const testServer = useMcpStore((s) => s.testServer);
  const setTheme = useMcpStore((s) => s.setTheme);

  const handleAdd = (server: ChatMcpServer) => {
    addServer(server);
    setShowAddForm(false);
  };

  const handleUpdate = (server: ChatMcpServer) => {
    updateServer(server.id, server);
    setEditingServer(null);
  };

  const handleDelete = (id: string) => {
    if (confirm("确定要删除这个 MCP Server 吗？")) {
      deleteServer(id);
    }
  };

  const handleTest = async (server: ChatMcpServer) => {
    setTestResult((prev) => ({ ...prev, [server.id]: null }));
    try {
      await testServer(server);
      setTestResult((prev) => ({ ...prev, [server.id]: "success" }));
    } catch {
      setTestResult((prev) => ({ ...prev, [server.id]: "error" }));
    }
    setTimeout(() => {
      setTestResult((prev) => ({ ...prev, [server.id]: null }));
    }, 3000);
  };

  const renderMcpTab = () => (
    <div className="flex-1 overflow-y-auto">
      <div className="flex items-center justify-between mb-4">
        <h2 className="text-sm font-medium text-ink-200">MCP Servers</h2>
        <button
          onClick={() => {
            setShowAddForm(true);
            setEditingServer(null);
          }}
          disabled={saving}
          className="flex items-center gap-1.5 px-3 py-1.5 text-xs text-ink-300 hover:text-ink-100 bg-ink-800 hover:bg-ink-700 rounded-md transition-colors disabled:opacity-50"
        >
          <svg className="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <line x1="12" y1="5" x2="12" y2="19" />
            <line x1="5" y1="12" x2="19" y2="12" />
          </svg>
          添加 Server
        </button>
      </div>

      {showAddForm && (
        <div className="mb-4">
          <McpServerForm
            onSubmit={handleAdd}
            onCancel={() => setShowAddForm(false)}
          />
        </div>
      )}

      {editingServer && (
        <div className="mb-4">
          <McpServerForm
            server={editingServer}
            onSubmit={handleUpdate}
            onCancel={() => setEditingServer(null)}
          />
        </div>
      )}

      {loading ? (
        <div className="flex items-center justify-center py-8">
          <svg className="w-5 h-5 text-ink-400 animate-spin" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <line x1="12" y1="2" x2="12" y2="6" />
            <line x1="12" y1="18" x2="12" y2="22" />
            <line x1="4.93" y1="4.93" x2="7.76" y2="7.76" />
            <line x1="16.24" y1="16.24" x2="19.07" y2="19.07" />
            <line x1="2" y1="12" x2="6" y2="12" />
            <line x1="18" y1="12" x2="22" y2="12" />
            <line x1="6.24" y1="16.24" x2="4.93" y2="19.07" />
            <line x1="19.07" y1="4.93" x2="16.24" y2="7.76" />
          </svg>
        </div>
      ) : servers.length === 0 ? (
        <div className="rounded-lg border border-dashed border-ink-700 p-6 text-center">
          <div className="text-ink-500 text-sm mb-2">暂无 MCP Server</div>
          <div className="text-ink-600 text-xs">点击上方按钮添加</div>
        </div>
      ) : (
        <div className="space-y-2">
          {servers.map((server) => {
            const state = states[server.id];
            const status = getStatusIcon(state);
            const testStatus = testResult[server.id];

            return (
              <div
                key={server.id}
                className="bg-ink-900 border border-ink-800 rounded-lg p-3"
              >
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-3">
                    <svg className={`w-4 h-4 ${status.color}`} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                      {status.icon === "circle-dot" && <circle cx="12" cy="12" r="10" />}
                      {status.icon === "loader-2" && (
                        <>
                          <line x1="12" y1="2" x2="12" y2="6" />
                          <line x1="12" y1="18" x2="12" y2="22" />
                          <line x1="4.93" y1="4.93" x2="7.76" y2="7.76" />
                          <line x1="16.24" y1="16.24" x2="19.07" y2="19.07" />
                          <line x1="2" y1="12" x2="6" y2="12" />
                          <line x1="18" y1="12" x2="22" y2="12" />
                          <line x1="6.24" y1="16.24" x2="4.93" y2="19.07" />
                          <line x1="19.07" y1="4.93" x2="16.24" y2="7.76" />
                        </>
                      )}
                      {status.icon === "check-circle-2" && (
                        <>
                          <circle cx="12" cy="12" r="10" />
                          <polyline points="16 10 10 16 8 14" />
                        </>
                      )}
                      {status.icon === "alert-circle" && (
                        <>
                          <circle cx="12" cy="12" r="10" />
                          <line x1="12" y1="8" x2="12" y2="12" />
                          <line x1="12" y1="16" x2="12.01" y2="16" />
                        </>
                      )}
                    </svg>
                    <div>
                      <div className="text-sm text-ink-200">{server.name}</div>
                      <div className="text-xs text-ink-500">{server.id}</div>
                    </div>
                  </div>

                  <div className="flex items-center gap-2">
                    <span className={`text-[10px] px-1.5 py-0.5 rounded ${
                      status.color === "text-green-400" ? "bg-green-500/10 text-green-400" :
                      status.color === "text-red-400" ? "bg-red-500/10 text-red-400" :
                      status.color === "text-blue-400" ? "bg-blue-500/10 text-blue-400" :
                      "bg-ink-800 text-ink-500"
                    }`}>
                      {status.label}
                    </span>

                    <label className="relative inline-flex items-center cursor-pointer">
                      <input
                        type="checkbox"
                        checked={server.enabled}
                        onChange={() => toggleServerEnabled(server.id)}
                        className="sr-only peer"
                      />
                      <div className="w-7 h-3.5 bg-ink-700 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-3 after:w-3 after:transition-all peer-checked:bg-brand-500" />
                    </label>

                    <button
                      onClick={() => handleTest(server)}
                      disabled={testStatus !== null || saving}
                      className="p-1.5 rounded-md text-ink-500 hover:bg-ink-800 hover:text-ink-300 transition-colors disabled:opacity-50"
                      title="测试连接"
                    >
                      {testStatus === "success" ? (
                        <svg className="w-3.5 h-3.5 text-green-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                          <polyline points="20 6 9 17 4 12" />
                        </svg>
                      ) : testStatus === "error" ? (
                        <svg className="w-3.5 h-3.5 text-red-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                          <line x1="18" y1="6" x2="6" y2="18" />
                          <line x1="6" y1="6" x2="18" y2="18" />
                        </svg>
                      ) : (
                        <svg className="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                          <path d="M21 12a9 9 0 0 0-9-9 9.75 9.75 0 0 0-6.74 2.74L3 8" />
                          <path d="M3 3v5h5" />
                          <path d="M3 12a9 9 0 0 0 9 9 9.75 9.75 0 0 0 6.74-2.74L21 16" />
                          <path d="M16 21h5v-5" />
                        </svg>
                      )}
                    </button>

                    <button
                      onClick={() => setEditingServer(server)}
                      className="p-1.5 rounded-md text-ink-500 hover:bg-ink-800 hover:text-ink-300 transition-colors"
                      title="编辑"
                    >
                      <svg className="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                        <path d="M17 3a2.85 2.83 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z" />
                        <path d="m15 5 4 4" />
                      </svg>
                    </button>

                    <button
                      onClick={() => handleDelete(server.id)}
                      className="p-1.5 rounded-md text-ink-500 hover:bg-red-500/10 hover:text-red-400 transition-colors"
                      title="删除"
                    >
                      <svg className="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                        <path d="M3 6h18" />
                        <path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6" />
                        <path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2" />
                      </svg>
                    </button>
                  </div>
                </div>

                <div className="mt-2 flex flex-wrap gap-2 text-[10px] text-ink-500">
                  <span className="bg-ink-800/50 px-1.5 py-0.5 rounded">
                    {server.command}
                    {server.args.length > 0 && ` ${server.args.slice(0, 2).join(" ")}${server.args.length > 2 ? "..." : ""}`}
                  </span>
                  {server.cwd && (
                    <span className="bg-ink-800/50 px-1.5 py-0.5 rounded">
                      cwd: {server.cwd}
                    </span>
                  )}
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );

  const renderLlmTab = () => (
    <div className="flex-1 flex items-center justify-center">
      <div className="text-center">
        <div className="text-ink-500 text-sm mb-2">LLM Provider 配置</div>
        <div className="text-ink-600 text-xs">目前通过 .env 文件配置，后续将支持 UI 配置</div>
      </div>
    </div>
  );

  const renderAppTab = () => (
    <div className="flex-1 overflow-y-auto">
      <div className="space-y-4">
        <div>
          <h2 className="text-sm font-medium text-ink-200 mb-3">主题</h2>
          <div className="flex items-center gap-4">
            <button
              onClick={() => setTheme("dark")}
              className={`flex items-center gap-2 px-4 py-2 rounded-lg border transition-all ${
                theme === "dark"
                  ? "border-brand-500 bg-brand-500/10 text-ink-100"
                  : "border-ink-700 bg-ink-800 text-ink-400 hover:border-ink-600"
              }`}
            >
              <svg className="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z" />
              </svg>
              <span className="text-xs">暗色</span>
            </button>
            <button
              onClick={() => setTheme("light")}
              className={`flex items-center gap-2 px-4 py-2 rounded-lg border transition-all ${
                theme === "light"
                  ? "border-brand-500 bg-brand-500/10 text-ink-100"
                  : "border-ink-700 bg-ink-800 text-ink-400 hover:border-ink-600"
              }`}
            >
              <svg className="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <circle cx="12" cy="12" r="5" />
                <line x1="12" y1="1" x2="12" y2="3" />
                <line x1="12" y1="21" x2="12" y2="23" />
                <line x1="4.22" y1="4.22" x2="5.64" y2="5.64" />
                <line x1="18.36" y1="18.36" x2="19.78" y2="19.78" />
                <line x1="1" y1="12" x2="3" y2="12" />
                <line x1="21" y1="12" x2="23" y2="12" />
                <line x1="4.22" y1="19.78" x2="5.64" y2="18.36" />
                <line x1="18.36" y1="5.64" x2="19.78" y2="4.22" />
              </svg>
              <span className="text-xs">亮色</span>
            </button>
          </div>
        </div>

        <div className="pt-4 border-t border-ink-800">
          <h2 className="text-sm font-medium text-ink-200 mb-2">关于</h2>
          <div className="text-xs text-ink-500 space-y-1">
            <div>Smart CodeAgent v0.1.0</div>
            <div>基于 Tauri 2 + React 19 构建</div>
          </div>
        </div>
      </div>
    </div>
  );

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm">
      <div className="w-full max-w-2xl max-h-[80vh] bg-ink-900 border border-ink-800 rounded-xl flex flex-col shadow-2xl">
        <div className="flex items-center justify-between px-4 py-3 border-b border-ink-800">
          <div className="flex items-center gap-2">
            <svg className="w-4 h-4 text-ink-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <circle cx="12" cy="12" r="3" />
              <path d="M12 1v6m0 6v6m4.22-10.22l4.24-4.24M6.34 6.34L2.1 2.1m17.9 10.9h-6m-6 0H1.9m17.8 0h.01M16.24 17.66l4.24 4.24M6.34 17.66l-4.24 4.24" />
            </svg>
            <span className="text-sm font-medium text-ink-200">Settings</span>
          </div>
          <button
            onClick={onClose}
            className="p-1.5 rounded-md text-ink-500 hover:bg-ink-800 hover:text-ink-300 transition-colors"
          >
            <svg className="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <line x1="18" y1="6" x2="6" y2="18" />
              <line x1="6" y1="6" x2="18" y2="18" />
            </svg>
          </button>
        </div>

        <div className="flex border-b border-ink-800">
          <button
            onClick={() => setActiveTab("mcp")}
            className={`flex-1 px-4 py-2.5 text-xs font-medium transition-colors ${
              activeTab === "mcp"
                ? "text-ink-200 border-b-2 border-brand-500"
                : "text-ink-500 hover:text-ink-300"
            }`}
          >
            MCP Servers
          </button>
          <button
            onClick={() => setActiveTab("llm")}
            className={`flex-1 px-4 py-2.5 text-xs font-medium transition-colors ${
              activeTab === "llm"
                ? "text-ink-200 border-b-2 border-brand-500"
                : "text-ink-500 hover:text-ink-300"
            }`}
          >
            LLM Provider
          </button>
          <button
            onClick={() => setActiveTab("app")}
            className={`flex-1 px-4 py-2.5 text-xs font-medium transition-colors ${
              activeTab === "app"
                ? "text-ink-200 border-b-2 border-brand-500"
                : "text-ink-500 hover:text-ink-300"
            }`}
          >
            外观
          </button>
        </div>

        <div className="flex-1 overflow-hidden p-4">
          {activeTab === "mcp" ? renderMcpTab() : activeTab === "llm" ? renderLlmTab() : renderAppTab()}
        </div>
      </div>
    </div>
  );
}
