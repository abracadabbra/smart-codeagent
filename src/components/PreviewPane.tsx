import { useMemo } from "react";
import { useChatStore } from "@/stores/chatStore";
import { ToolCallCard } from "@/components/chat/ToolCallCard";
import { AskUserPromptCard } from "@/components/chat/AskUserPromptCard";

/**
 * 右栏：工具调用预览面板。
 *
 * 显示当前所有 run 的 ToolCallRecord（按 startedAt 排序）。
 * ApprovalDialog 是 modal，挂在 body 上而非本栏内（固定定位）；
 * AskUserPromptCard 作为 inline 卡片插在最上方。
 *
 * 借 Kivio `PreviewPane.tsx` 的"右栏聚合工具事件"思路。
 */
export function PreviewPane() {
  const toolRecordsByRun = useChatStore((s) => s.toolRecordsByRun);

  const records = useMemo(() => {
    const all = Object.values(toolRecordsByRun).flatMap((byId) =>
      Object.values(byId),
    );
    return all.sort(
      (a, b) => (a.startedAt ?? 0) - (b.startedAt ?? 0),
    );
  }, [toolRecordsByRun]);

  return (
    <aside className="w-80 shrink-0 border-l border-ink-700 bg-ink-800/30 flex flex-col">
      <div className="px-3 py-2 border-b border-ink-700 text-xs font-medium text-ink-200">
        Preview
      </div>

      <div className="flex-1 overflow-y-auto p-3 space-y-2">
        <AskUserPromptCard />

        {records.length === 0 ? (
          <div className="rounded-md border border-dashed border-ink-600 p-3 text-center text-ink-400 text-xs">
            工具调用将显示在此处
          </div>
        ) : (
          records.map((r) => <ToolCallCard key={r.id} record={r} />)
        )}
      </div>
    </aside>
  );
}
