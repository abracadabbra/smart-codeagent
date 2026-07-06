import { useState } from "react";
import { useAgentStore } from "@/stores/agentStore";
import { approveTool } from "@/hooks/useAgentEvents";

/**
 * 工具批准弹窗：modal 形态，用户必须选批准 / 拒绝。
 * 由 `agent:approval_request` 事件触发，store 持有 latest request。
 *
 * 借 Kivio `ApprovalModal.tsx` 的强制阻断交互（点之前别处不响应）。
 */
export function ApprovalDialog() {
  const req = useAgentStore((s) => s.approvalRequest);
  const clearApproval = useAgentStore((s) => s.clearApproval);
  const [sending, setSending] = useState(false);

  if (!req) return null;

  const onRespond = async (allow: boolean) => {
    if (sending) return;
    setSending(true);
    try {
      // eslint-disable-next-line no-console
      console.log("[ApprovalDialog] responding:", {
        conversationId: req.conversationId,
        approvalId: req.approvalId,
        allow,
      });
      await approveTool(req.conversationId, req.approvalId, allow);
      // eslint-disable-next-line no-console
      console.log("[ApprovalDialog] approveTool call succeeded");
    } catch (err) {
      // eslint-disable-next-line no-console
      console.error("[ApprovalDialog] approveTool failed:", err);
    } finally {
      setSending(false);
      clearApproval();
    }
  };

  let argsPretty = req.arguments;
  try {
    argsPretty = JSON.stringify(JSON.parse(req.arguments), null, 2);
  } catch {
    // keep raw
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm">
      <div className="w-[440px] max-w-[90vw] rounded-xl border border-ink-600 bg-ink-800 shadow-2xl">
        <div className="px-4 py-3 border-b border-ink-700 flex items-center gap-2">
          <span className="text-orange-400 text-sm font-medium">
            ⚠ 工具调用需要批准
          </span>
          {req.sensitive && (
            <span className="text-[10px] px-1.5 py-0.5 rounded bg-orange-500/20 text-orange-300 border border-orange-500/40">
              sensitive
            </span>
          )}
        </div>

        <div className="px-4 py-3 space-y-3 text-sm">
          <div>
            <div className="text-ink-400 text-[10px] uppercase tracking-wide mb-1">
              tool
            </div>
            <div className="font-mono text-ink-100">{req.toolName}</div>
          </div>

          {argsPretty && (
            <div>
              <div className="text-ink-400 text-[10px] uppercase tracking-wide mb-1">
                arguments
              </div>
              <pre className="bg-ink-900/80 rounded p-2 overflow-x-auto text-ink-100 font-mono text-[11px] leading-relaxed max-h-48 overflow-y-auto">
                {argsPretty}
              </pre>
            </div>
          )}
        </div>

        <div className="px-4 py-3 border-t border-ink-700 flex justify-end gap-2">
          <button
            type="button"
            disabled={sending}
            onClick={() => void onRespond(false)}
            className="px-3 py-1.5 rounded-lg bg-ink-700 hover:bg-ink-600 disabled:opacity-50 text-ink-100 text-sm font-medium transition-colors"
          >
            拒绝
          </button>
          <button
            type="button"
            disabled={sending}
            onClick={() => void onRespond(true)}
            className="px-3 py-1.5 rounded-lg bg-emerald-600 hover:bg-emerald-500 disabled:opacity-50 text-white text-sm font-medium transition-colors"
          >
            {sending ? "处理中…" : "批准"}
          </button>
        </div>
      </div>
    </div>
  );
}
