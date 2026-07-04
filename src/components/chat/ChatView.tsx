import { useEffect, useRef } from "react";
import { useChatStore } from "@/stores/chatStore";
import { useAgentStore } from "@/stores/agentStore";
import { MessageBubble } from "./MessageBubble";
import { InputBar } from "./InputBar";

/**
 * 中间栏：对话流 + 底部输入框。
 * 三栏容器放在 App.tsx，这里只关注中栏自身。
 */
export function ChatView() {
  const messages = useChatStore((s) => s.messages);
  const agentState = useAgentStore((s) => s.state);
  const lastError = useAgentStore((s) => s.lastError);

  const scrollRef = useRef<HTMLDivElement>(null);

  // 新消息追加时自动滚到底
  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
  }, [messages]);

  return (
    <div className="flex flex-col h-full bg-ink-900">
      {/* 对话区 */}
      <div ref={scrollRef} className="flex-1 overflow-y-auto px-6 py-4">
        {messages.length === 0 ? (
          <div className="h-full flex items-center justify-center text-ink-300 text-sm">
            开始对话吧…
          </div>
        ) : (
          messages.map((m) => <MessageBubble key={m.id} message={m} />)
        )}

        {lastError && messages.length === 0 && (
          <div className="text-red-400 text-sm text-center mt-4">{lastError}</div>
        )}
      </div>

      {/* 状态栏 */}
      <div className="px-6 py-1 border-t border-ink-700 text-xs text-ink-300 flex items-center gap-2">
        <span
          className={[
            "inline-block w-1.5 h-1.5 rounded-full",
            agentState === "Idle"
              ? "bg-ink-400"
              : agentState === "Stream"
                ? "bg-blue-500 animate-pulse"
                : "bg-amber-500",
          ].join(" ")}
        />
        <span>{agentState}</span>
        {lastError && agentState === "Idle" && (
          <span className="text-red-400">· {lastError}</span>
        )}
      </div>

      {/* 输入框 */}
      <InputBar />
    </div>
  );
}