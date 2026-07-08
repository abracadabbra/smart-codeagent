import { memo } from "react";
import { StreamingText } from "./StreamingText";
import { MarkdownMessage } from "./MarkdownMessage";
import { useChatStore } from "@/stores/chatStore";
import type { Message } from "@/types/message";

interface MessageBubbleProps {
  message: Message;
  messageIndex: number;
}

/**
 * 消息气泡（memoized）：
 * - 用户：右侧对齐，品牌色胶囊背景，纯文本
 * - 助手：左侧对齐，无背景/无边框，Markdown 渲染
 *
 * 使用 React.memo 避免流式输出时已完成的消息被重渲染。
 */
export const MessageBubble = memo(function MessageBubble({ message, messageIndex }: MessageBubbleProps) {
  const isUser = message.role === "user";
  const searchResults = useChatStore((s) => s.searchResults);
  const searchCurrentIndex = useChatStore((s) => s.searchCurrentIndex);
  const isMatch = searchResults.includes(messageIndex);
  const isCurrent = searchResults[searchCurrentIndex] === messageIndex;

  const highlightClass = isCurrent
    ? "ring-2 ring-brand-400 ring-offset-2 ring-offset-ink-950"
    : isMatch
      ? "ring-1 ring-amber-400/50"
      : "";

  return (
    <div
      data-message-index={messageIndex}
      className={`flex ${isUser ? "justify-end" : "justify-start"} my-3.5 transition-all ${highlightClass}`}
    >
      {isUser ? (
        <div className="max-w-[75%] rounded-2xl rounded-tr-sm px-5 py-3.5 bg-accent text-white text-base leading-[1.65] shadow-sm">
          <div className="whitespace-pre-wrap break-words selection-brand">
            {message.content}
          </div>
          {message.status === "error" && message.error && (
            <div className="mt-2 text-xs text-red-200/80 border-t border-red-400/30 pt-2">
              {message.error}
            </div>
          )}
        </div>
      ) : (
        <div className="max-w-[88%]">
          <div className="rounded-2xl rounded-tl-sm px-5 py-3 bg-ink-900/60">
            {message.status === "streaming" ? (
              <StreamingText text={message.content} streaming={true} />
            ) : (
              <MarkdownMessage content={message.content} />
            )}
            {message.status === "error" && message.error && (
              <div className="mt-2 text-xs text-red-400/80 border-l-2 border-red-500/50 pl-3 py-1">
                {message.error}
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
});
