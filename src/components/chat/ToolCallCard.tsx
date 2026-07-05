import { useState } from "react";
import type { ToolCallRecord, ToolCallStatus } from "@/types/tool";

interface ToolCallCardProps {
  record: ToolCallRecord;
}

const STATUS_STYLES: Record<ToolCallStatus, string> = {
  Pending: "bg-amber-500/20 text-amber-300 border-amber-500/40",
  Running: "bg-blue-500/20 text-blue-300 border-blue-500/40 animate-pulse",
  Success: "bg-emerald-500/20 text-emerald-300 border-emerald-500/40",
  Error: "bg-red-500/20 text-red-300 border-red-500/40",
  Cancelled: "bg-ink-500/20 text-ink-300 border-ink-500/40",
  Skipped: "bg-ink-500/20 text-ink-300 border-ink-500/40",
};

function formatDuration(ms?: number): string | null {
  if (ms == null) return null;
  if (ms < 1000) return `${ms}ms`;
  return `${(ms / 1000).toFixed(2)}s`;
}

function prettyJson(raw: string): string {
  if (!raw) return "";
  try {
    return JSON.stringify(JSON.parse(raw), null, 2);
  } catch {
    return raw;
  }
}

/**
 * 单个工具调用的折叠卡片：header 显示 name / status / duration / round，
 * 展开后显示 arguments (pretty JSON) + result preview + error。
 *
 * 借 Kivio `ToolCallBlock.tsx` 的折叠交互，但精简到只读 + 不嵌套子卡片。
 */
export function ToolCallCard({ record }: ToolCallCardProps) {
  const [open, setOpen] = useState(false);

  const duration = formatDuration(record.durationMs);
  const argsPretty = prettyJson(record.arguments);

  return (
    <div className="rounded-lg border border-ink-700 bg-ink-800/60 overflow-hidden text-xs">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="w-full flex items-center gap-2 px-3 py-2 hover:bg-ink-700/40 transition-colors text-left"
      >
        <span className="text-ink-300 select-none">{open ? "▼" : "▶"}</span>
        <span className="font-mono font-medium text-ink-100">
          {record.name}
        </span>
        {record.sensitive && (
          <span className="text-[10px] px-1.5 py-0.5 rounded bg-orange-500/20 text-orange-300 border border-orange-500/40">
            sensitive
          </span>
        )}
        <span
          className={[
            "text-[10px] px-1.5 py-0.5 rounded border font-medium",
            STATUS_STYLES[record.status],
          ].join(" ")}
        >
          {record.status}
        </span>
        {duration && (
          <span className="text-ink-300 ml-auto tabular-nums">{duration}</span>
        )}
        <span className="text-ink-400 text-[10px]">r{record.round}</span>
      </button>

      {open && (
        <div className="border-t border-ink-700 px-3 py-2 space-y-2">
          {argsPretty && (
            <div>
              <div className="text-ink-400 mb-1 text-[10px] uppercase tracking-wide">
                arguments
              </div>
              <pre className="bg-ink-900/80 rounded p-2 overflow-x-auto text-ink-100 font-mono text-[11px] leading-relaxed">
                {argsPretty}
              </pre>
            </div>
          )}

          {record.resultPreview && (
            <div>
              <div className="text-ink-400 mb-1 text-[10px] uppercase tracking-wide">
                result
              </div>
              <pre className="bg-ink-900/80 rounded p-2 overflow-x-auto text-emerald-200/90 font-mono text-[11px] leading-relaxed max-h-48 overflow-y-auto">
                {record.resultPreview}
              </pre>
            </div>
          )}

          {record.error && (
            <div>
              <div className="text-red-400 mb-1 text-[10px] uppercase tracking-wide">
                error
              </div>
              <pre className="bg-red-950/40 border border-red-500/30 rounded p-2 overflow-x-auto text-red-200 font-mono text-[11px] leading-relaxed">
                {record.error}
              </pre>
            </div>
          )}

          {record.artifacts.length > 0 && (
            <div>
              <div className="text-ink-400 mb-1 text-[10px] uppercase tracking-wide">
                artifacts
              </div>
              <ul className="space-y-0.5 font-mono text-[11px] text-blue-300">
                {record.artifacts.map((p, i) => (
                  <li key={i} className="break-all">
                    {p}
                  </li>
                ))}
              </ul>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
