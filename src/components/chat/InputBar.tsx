import { useState, useRef, useCallback } from "react";
import { useChatStore } from "@/stores/chatStore";
import { useAgentStore } from "@/stores/agentStore";
import { sendMessage } from "@/hooks/useAgentEvents";

/**
 * 底部输入框：禁用条件 = Agent 不在 Idle。
 * 提交时：
 *   1. 在 chatStore 中创建 user + 占位 assistant 两条消息
 *   2. invoke('send_message') 启动 Rust Loop（前端生成 runId + generation，
 *      避免后端 race；generation 单调递增便于 cancel 判定）
 *   3. 清空输入框
 */
const genRef = { n: 0 };
const newRunId = () =>
  `run-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;

export function InputBar() {
  const [text, setText] = useState("");
  const sendingRef = useRef(false);

  const appendUserMessage = useChatStore((s) => s.appendUserMessage);
  const agentState = useAgentStore((s) => s.state);

  const disabled = agentState !== "Idle";

  const submit = useCallback(async () => {
    const trimmed = text.trim();
    if (!trimmed || sendingRef.current || disabled) return;
    sendingRef.current = true;

    try {
      const { assistantId } = appendUserMessage(trimmed);
      const runId = newRunId();
      const generation = ++genRef.n;
      setText("");
      await sendMessage({
        text: trimmed,
        assistantId,
        runId,
        generation,
      });
    } catch (err) {
      // eslint-disable-next-line no-console
      console.error("sendMessage failed:", err);
    } finally {
      sendingRef.current = false;
    }
  }, [text, disabled, appendUserMessage]);

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void submit();
    }
  };

  return (
    <div className="border-t border-ink-700 bg-ink-900/80 backdrop-blur px-4 py-3">
      <div className="flex items-end gap-3">
        <textarea
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={handleKeyDown}
          disabled={disabled}
          placeholder={disabled ? "Agent 正在处理…" : "输入消息，回车发送，Shift+Enter 换行"}
          rows={2}
          className="flex-1 resize-none rounded-xl bg-ink-800 px-3 py-2 text-sm text-ink-100 placeholder:text-ink-300 outline-none border border-ink-600 focus:border-blue-500 disabled:opacity-50"
        />
        <button
          type="button"
          onClick={() => void submit()}
          disabled={disabled || !text.trim()}
          className="px-4 py-2 rounded-xl bg-blue-600 hover:bg-blue-500 disabled:bg-ink-600 disabled:cursor-not-allowed text-ink-50 text-sm font-medium transition-colors"
        >
          发送
        </button>
      </div>
    </div>
  );
}