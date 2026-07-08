import { useEffect, useRef, useMemo } from "react";
import { useChatStore } from "@/stores/chatStore";
import { useAgentStore } from "@/stores/agentStore";
import { useSessionStore } from "@/stores/sessionStore";
import type { AgentState } from "@/types/agent";
import { VirtualMessageList } from "./VirtualMessageList";
import type { VirtualMessageListHandle } from "./VirtualMessageList";
import { InputBar } from "./InputBar";
import { SearchBar } from "./SearchBar";

function stateLabel(state: AgentState): string {
  switch (state) {
    case "Prepare":
      return "准备中";
    case "ToolLoop":
      return "工具调用中";
    case "Stream":
      return "流式输出中";
    case "Synthesis":
      return "合成中";
    case "Plain":
      return "生成中";
    case "RetryBackoff":
      return "请求失败，退避重试中";
    case "TrimContext":
      return "上下文过长，裁剪历史后重试";
    case "Stop":
      return "停止中";
    case "Idle":
    default:
      return "就绪";
  }
}

function stateColor(state: AgentState): string {
  switch (state) {
    case "Stream":
      return "bg-brand-500 animate-pulse";
    case "RetryBackoff":
    case "TrimContext":
      return "bg-yellow-400 animate-pulse";
    case "Idle":
      return "bg-ink-500";
    default:
      return "bg-amber-500";
  }
}

export function ChatView() {
  const messages = useChatStore((s) => s.messages);
  const agentState = useAgentStore((s) => s.state);
  const lastError = useAgentStore((s) => s.lastError);
  const activeSessionId = useSessionStore((s) => s.activeSessionId);
  const sessions = useSessionStore((s) => s.sessions);

  const activeSession = useMemo(
    () => sessions.find((s) => s.id === activeSessionId) ?? null,
    [sessions, activeSessionId],
  );

  const listRef = useRef<VirtualMessageListHandle>(null);
  const searchCurrentIndex = useChatStore((s) => s.searchCurrentIndex);
  const searchResults = useChatStore((s) => s.searchResults);

  // 只在消息数量变化或会话切换时滚动到底部，避免每个 token 都触发滚动
  const messageCount = messages.length;
  const lastMessageStatus = messages[messageCount - 1]?.status ?? "";

  useEffect(() => {
    const list = listRef.current;
    if (!list) return;
    requestAnimationFrame(() => {
      list.scrollToBottom("auto");
    });
     
  }, [messageCount, activeSessionId]);

  // 流式输出时，只在消息状态变化（如 pending→streaming→complete）时滚动，而非每个 token
  useEffect(() => {
    const list = listRef.current;
    if (!list) return;
    requestAnimationFrame(() => {
      list.scrollToBottom("smooth");
    });
     
  }, [lastMessageStatus]);

  // 搜索当前项变化时，滚动到对应消息
  const scrollToIndex =
    searchCurrentIndex >= 0 && searchResults.length > 0
      ? searchResults[searchCurrentIndex]
      : null;

  const hasMessages = messages.length > 0;

  return (
    <div className="flex flex-col h-full bg-ink-950 relative">
      {/* 对话区：没有消息时显示中央空态 */}
      <div className="flex-1 overflow-y-auto relative">
        {activeSession && hasMessages ? (
          <VirtualMessageList
            ref={listRef}
            messages={messages}
            scrollToIndex={scrollToIndex}
          />
        ) : (
          <div className="absolute inset-0 flex flex-col items-center justify-center px-6 animate-fade-in">
            {/* 大 Logo / 星球图标 */}
            <div className="relative mb-8">
              <div className="w-20 h-20 rounded-full bg-gradient-to-br from-ink-800 to-ink-900 border border-ink-700/80 flex items-center justify-center shadow-2xl">
                <svg
                  className="w-9 h-9 text-ink-400"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="1.5"
                >
                  <circle cx="12" cy="12" r="10" />
                  <path d="M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20" />
                  <path d="M2 12h20" />
                </svg>
              </div>
              <div className="absolute -bottom-1 -right-1 w-6 h-6 rounded-full bg-accent flex items-center justify-center">
                <svg className="w-3.5 h-3.5 text-white" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                  <path d="M12 2a10 10 0 0 1 0 20" />
                  <path d="M12 2a10 10 0 0 0 0 20" />
                </svg>
              </div>
            </div>

            <h1 className="text-3xl font-semibold text-ink-100 mb-3 tracking-tight">
              Quest on, hands off
            </h1>

            <div className="flex items-center gap-3 text-sm text-ink-400 mb-12">
              <span>运行于</span>
              <button className="flex items-center gap-1.5 px-2 py-1 rounded-md bg-ink-900 border border-ink-800 hover:border-ink-700 transition-colors">
                <svg className="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                  <path d="M4 20h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.93a2 2 0 0 1-1.66-.9l-.82-1.2A2 2 0 0 0 7.93 2H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2Z" />
                </svg>
                <span className="text-ink-200">smart-codeagent</span>
                <svg className="w-3 h-3 text-ink-500" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                  <polyline points="6 9 12 15 18 9" />
                </svg>
              </button>
              <button className="flex items-center gap-1.5 px-2 py-1 rounded-md bg-ink-900 border border-ink-800 hover:border-ink-700 transition-colors">
                <svg className="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                  <rect x="2" y="3" width="20" height="14" rx="2" ry="2" />
                  <line x1="8" y1="21" x2="16" y2="21" />
                  <line x1="12" y1="17" x2="12" y2="21" />
                </svg>
                <span className="text-ink-200">本地模式</span>
                <svg className="w-3 h-3 text-ink-500" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                  <polyline points="6 9 12 15 18 9" />
                </svg>
              </button>
            </div>

            {lastError && (
              <div className="text-red-400 text-sm text-center mt-4">{lastError}</div>
            )}
          </div>
        )}
      </div>

      {/* 底部输入卡片 */}
      <InputBar />

      {/* 消息搜索栏 */}
      <SearchBar />

      {/* 简洁状态提示 */}
      {hasMessages && (
        <div className="absolute top-4 right-4 flex items-center gap-2 text-[11px] text-ink-500">
          <span
            className={[
              "inline-block w-1.5 h-1.5 rounded-full",
              stateColor(agentState),
            ].join(" ")}
          />
          <span>{stateLabel(agentState)}</span>
        </div>
      )}
    </div>
  );
}
