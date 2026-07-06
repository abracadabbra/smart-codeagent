import { useState, useRef, useCallback, useEffect } from "react";
import { useChatStore } from "@/stores/chatStore";
import { useAgentStore } from "@/stores/agentStore";
import { useSessionStore } from "@/stores/sessionStore";
import { sendMessage, cancelRun } from "@/hooks/useAgentEvents";

const newRunId = () =>
  `run-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;

export function InputBar() {
  const [text, setText] = useState("");
  const [uiError, setUiError] = useState<string | null>(null);
  const sendingRef = useRef(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const appendUserMessage = useChatStore((s) => s.appendUserMessage);
  const setError = useChatStore((s) => s.setError);
  const agentState = useAgentStore((s) => s.state);
  const activeSessionId = useSessionStore((s) => s.activeSessionId);

  const isGenerating = agentState !== "Idle" && !!activeSessionId;
  const disabled = isGenerating;

  useEffect(() => {
    const el = textareaRef.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 160)}px`;
  }, [text]);

  const submit = useCallback(async () => {
    const trimmed = text.trim();
    if (!trimmed || sendingRef.current || disabled || !activeSessionId) {
      return;
    }
    sendingRef.current = true;
    setUiError(null);

    try {
      const { assistantId } = appendUserMessage(trimmed);
      const runId = newRunId();
      setText("");
      if (textareaRef.current) textareaRef.current.style.height = "auto";

      const result = await sendMessage({
        conversationId: activeSessionId,
        text: trimmed,
        runId,
        messageId: assistantId,
      });

      if (!result.success) {
        const errMsg = result.error || "发送失败";
        setUiError(errMsg);
        setError(errMsg);
      }
    } catch (err) {
      const errMsg = err instanceof Error ? err.message : String(err);
      setUiError(errMsg);
      setError(errMsg);
    } finally {
      sendingRef.current = false;
    }
  }, [text, disabled, appendUserMessage, activeSessionId, setError]);

  const handleStop = useCallback(() => {
    if (activeSessionId) {
      void cancelRun(activeSessionId);
    }
  }, [activeSessionId]);

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void submit();
    }
  };

  return (
    <div className="shrink-0 px-6 pb-8 pt-4">
      <div className="max-w-2xl mx-auto">
        {uiError && (
          <div className="mb-2 px-3 py-2 rounded-lg bg-red-950/40 border border-red-500/30 text-red-300 text-xs">
            {uiError}
          </div>
        )}

        <div className="relative bg-surface-raised rounded-2xl shadow-input-card border border-ink-800 transition-all focus-within:shadow-input-card-focus focus-within:border-ink-700">
          <textarea
            ref={textareaRef}
            value={text}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={handleKeyDown}
            disabled={disabled}
            placeholder="描述计划，@ 引用上下文，/ 使用命令"
            rows={1}
            className="w-full resize-none max-h-40 bg-transparent px-4 pt-4 pb-12 text-sm text-ink-100 placeholder:text-ink-600 outline-none disabled:opacity-50 selection-brand rounded-2xl"
          />

          {/* 底部工具栏 */}
          <div className="absolute bottom-2 left-2 right-2 flex items-center justify-between">
            <div className="flex items-center gap-1.5">
              <button
                type="button"
                className="flex items-center gap-1.5 px-2 py-1.5 rounded-lg text-xs text-ink-400 hover:bg-ink-800/60 hover:text-ink-200 transition-colors"
              >
                <svg className="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                  <path d="M12 2a4 4 0 0 1 4 4c0 2.21-1.79 4-4 4s-4-1.79-4-4a4 4 0 0 1 4-4z" />
                  <path d="M2 20c0-4 4-6 10-6s10 2 10 6" />
                </svg>
                <span>智能体</span>
                <svg className="w-3 h-3 text-ink-600" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                  <polyline points="6 9 12 15 18 9" />
                </svg>
              </button>

              <button
                type="button"
                className="flex items-center gap-1.5 px-2 py-1.5 rounded-lg text-xs text-ink-400 hover:bg-ink-800/60 hover:text-ink-200 transition-colors"
              >
                <span>Auto</span>
                <svg className="w-3 h-3 text-ink-600" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                  <polyline points="6 9 12 15 18 9" />
                </svg>
              </button>
            </div>

            <div className="flex items-center gap-1.5">
              {isGenerating ? (
                /* 停止按钮：agent 运行时显示 */
                <button
                  type="button"
                  onClick={handleStop}
                  className="flex items-center justify-center w-8 h-8 rounded-lg bg-ink-700 hover:bg-red-900/60 text-ink-200 hover:text-red-300 transition-colors"
                  title="停止生成"
                >
                  <svg className="w-3.5 h-3.5" viewBox="0 0 24 24" fill="currentColor">
                    <rect x="6" y="6" width="12" height="12" rx="1.5" />
                  </svg>
                </button>
              ) : (
                <button
                  type="button"
                  disabled={disabled || !text.trim()}
                  onClick={() => void submit()}
                  className="flex items-center justify-center w-8 h-8 rounded-lg bg-accent hover:bg-accent-hover disabled:bg-ink-800 disabled:text-ink-600 text-white transition-colors"
                >
                  <svg className="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                    <line x1="22" y1="2" x2="11" y2="13" />
                    <polygon points="22 2 15 22 11 13 2 9 22 2" />
                  </svg>
                </button>
              )}
            </div>
          </div>
        </div>

        <div className="mt-2 text-[10px] text-ink-600 text-center">
          Agent 可能会调用本地工具，请确认操作安全。
        </div>
      </div>
    </div>
  );
}
