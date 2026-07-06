import { useMemo } from "react";
import { useChatStore } from "@/stores/chatStore";
import { ToolCallCard } from "@/components/chat/ToolCallCard";
import { AskUserPromptCard } from "@/components/chat/AskUserPromptCard";

/**
 * 右栏：可折叠的工具调用预览面板。
 * 默认收起；通过 ChatView 顶部按钮或本栏按钮切换。
 */
export function PreviewPane() {
  const toolRecordsByRun = useChatStore((s) => s.toolRecordsByRun);
  const previewOpen = useChatStore((s) => s.previewOpen);
  const togglePreview = useChatStore((s) => s.togglePreview);

  const records = useMemo(() => {
    const all = Object.values(toolRecordsByRun).flatMap((byId) =>
      Object.values(byId),
    );
    return all.sort(
      (a, b) => (a.startedAt ?? 0) - (b.startedAt ?? 0),
    );
  }, [toolRecordsByRun]);

  if (!previewOpen) {
    return (
      <aside className="w-10 shrink-0 border-l border-ink-800/60 bg-ink-900/30 flex flex-col items-center py-3">
        <button
          type="button"
          onClick={togglePreview}
          title="展开 Preview"
          className="p-1.5 rounded-lg text-ink-500 hover:bg-ink-800 hover:text-ink-200 transition-colors"
        >
          <svg className="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <rect x="2" y="3" width="20" height="14" rx="2" ry="2" />
            <line x1="8" y1="21" x2="16" y2="21" />
            <line x1="12" y1="17" x2="12" y2="21" />
          </svg>
        </button>
      </aside>
    );
  }

  return (
    <aside className="w-72 shrink-0 border-l border-ink-800/60 bg-ink-900/30 flex flex-col">
      <div className="h-12 shrink-0 px-3 flex items-center justify-between border-b border-ink-800/60 bg-ink-900/40">
        <span className="text-xs font-medium text-ink-200">Preview</span>
        <button
          type="button"
          onClick={togglePreview}
          title="收起 Preview"
          className="p-1.5 rounded-md text-ink-500 hover:bg-ink-800 hover:text-ink-200 transition-colors"
        >
          <svg className="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="M4 14h6v6H4zM4 4h6v6H4zM14 4h6v6h-6zM14 14h6v6h-6z" />
          </svg>
        </button>
      </div>

      <div className="flex-1 overflow-y-auto p-3 space-y-2">
        <AskUserPromptCard />

        {records.length === 0 ? (
          <div className="rounded-lg border border-dashed border-ink-700 p-4 text-center text-ink-500 text-xs">
            工具调用将显示在此处
          </div>
        ) : (
          records.map((r) => <ToolCallCard key={r.id} record={r} />)
        )}
      </div>
    </aside>
  );
}
