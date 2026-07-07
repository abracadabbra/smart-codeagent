import { useState, useEffect } from "react";
import type { ChatMcpServer } from "@/types/mcp";

interface McpServerFormProps {
  server?: ChatMcpServer | null;
  onSubmit: (server: ChatMcpServer) => void;
  onCancel: () => void;
}

export function McpServerForm({ server, onSubmit, onCancel }: McpServerFormProps) {
  const [id, setId] = useState("");
  const [name, setName] = useState("");
  const [command, setCommand] = useState("");
  const [args, setArgs] = useState("");
  const [cwd, setCwd] = useState("");
  const [enabled, setEnabled] = useState(true);

  useEffect(() => {
    if (server) {
      setId(server.id);
      setName(server.name);
      setCommand(server.command);
      setArgs(server.args.join("\n"));
      setCwd(server.cwd || "");
      setEnabled(server.enabled);
    }
  }, [server]);

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    const parsedArgs = args.split("\n").map((a) => a.trim()).filter((a) => a);
    onSubmit({
      id: id.trim(),
      name: name.trim(),
      enabled,
      transport: "stdio",
      command: command.trim(),
      args: parsedArgs,
      env: {},
      cwd: cwd.trim() || null,
      enabledTools: [],
    });
  };

  return (
    <div className="bg-ink-900 border border-ink-800 rounded-lg p-4">
      <h3 className="text-sm font-medium text-ink-200 mb-4">
        {server ? "编辑 MCP Server" : "添加 MCP Server"}
      </h3>
      <form onSubmit={handleSubmit} className="space-y-3">
        <div>
          <label className="block text-[10px] font-medium text-ink-500 uppercase tracking-wider mb-1">
            ID <span className="text-red-400">*</span>
          </label>
          <input
            type="text"
            value={id}
            onChange={(e) => setId(e.target.value)}
            placeholder="唯一标识符，如 fs"
            className="w-full bg-ink-950 border border-ink-800 rounded-md px-3 py-2 text-sm text-ink-100 placeholder-ink-600 focus:outline-none focus:border-brand-500 focus:ring-1 focus:ring-brand-500/20"
            required
          />
        </div>

        <div>
          <label className="block text-[10px] font-medium text-ink-500 uppercase tracking-wider mb-1">
            名称 <span className="text-red-400">*</span>
          </label>
          <input
            type="text"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="显示名称，如 Filesystem"
            className="w-full bg-ink-950 border border-ink-800 rounded-md px-3 py-2 text-sm text-ink-100 placeholder-ink-600 focus:outline-none focus:border-brand-500 focus:ring-1 focus:ring-brand-500/20"
            required
          />
        </div>

        <div>
          <label className="block text-[10px] font-medium text-ink-500 uppercase tracking-wider mb-1">
            命令 <span className="text-red-400">*</span>
          </label>
          <input
            type="text"
            value={command}
            onChange={(e) => setCommand(e.target.value)}
            placeholder="如 npx, node, python3"
            className="w-full bg-ink-950 border border-ink-800 rounded-md px-3 py-2 text-sm text-ink-100 placeholder-ink-600 focus:outline-none focus:border-brand-500 focus:ring-1 focus:ring-brand-500/20"
            required
          />
        </div>

        <div>
          <label className="block text-[10px] font-medium text-ink-500 uppercase tracking-wider mb-1">
            参数
          </label>
          <textarea
            value={args}
            onChange={(e) => setArgs(e.target.value)}
            placeholder="每行一个参数，如：&#10;-y&#10;@modelcontextprotocol/server-filesystem&#10;/tmp"
            rows={3}
            className="w-full bg-ink-950 border border-ink-800 rounded-md px-3 py-2 text-sm text-ink-100 placeholder-ink-600 focus:outline-none focus:border-brand-500 focus:ring-1 focus:ring-brand-500/20 resize-none"
          />
        </div>

        <div>
          <label className="block text-[10px] font-medium text-ink-500 uppercase tracking-wider mb-1">
            工作目录（可选）
          </label>
          <input
            type="text"
            value={cwd}
            onChange={(e) => setCwd(e.target.value)}
            placeholder="留空则继承父进程"
            className="w-full bg-ink-950 border border-ink-800 rounded-md px-3 py-2 text-sm text-ink-100 placeholder-ink-600 focus:outline-none focus:border-brand-500 focus:ring-1 focus:ring-brand-500/20"
          />
        </div>

        <div className="flex items-center gap-2">
          <input
            type="checkbox"
            id="enabled"
            checked={enabled}
            onChange={(e) => setEnabled(e.target.checked)}
            className="w-4 h-4 rounded border-ink-700 bg-ink-800 text-brand-500 focus:ring-brand-500/20"
          />
          <label htmlFor="enabled" className="text-sm text-ink-300">
            启用
          </label>
        </div>

        <div className="flex justify-end gap-2 pt-2">
          <button
            type="button"
            onClick={onCancel}
            className="px-3 py-1.5 text-sm text-ink-400 hover:text-ink-200 hover:bg-ink-800 rounded-md transition-colors"
          >
            取消
          </button>
          <button
            type="submit"
            className="px-3 py-1.5 text-sm bg-brand-500 text-white rounded-md hover:bg-brand-600 transition-colors"
          >
            {server ? "保存" : "添加"}
          </button>
        </div>
      </form>
    </div>
  );
}
