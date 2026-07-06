import { memo } from "react";
import { StreamingText } from "./StreamingText";
import { MarkdownMessage } from "./MarkdownMessage";
import type { Message } from "@/types/message";

interface MessageBubbleProps {
  message: Message;
}

/**
 * 消息气泡（memoized）：
 * - 用户：右侧对齐，品牌色胶囊背景，纯文本
 * - 助手：左侧对齐，无背景/无边框，Markdown 渲染
 *
 * 使用 React.memo 避免流式输出时已完成的消息被重渲染。
 */
export const MessageBubble = memo(function MessageBubble({ message }: MessageBubbleProps) {
  const isUser = message.role === "user";

  return (
    <div className={`flex ${isUser ? "justify-end" : "justify-start"} my-3.5`}>
      {isUser ? (
        <div className="max-w-[75%] rounded-2xl rounded-tr-sm px-4.5 py-3 bg-brand-600 text-white text-base leading-[1.65] shadow-sm">
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
          <div className="rounded-2xl rounded-tl-sm px-4.5 py-3 bg-ink-900/60 border border-ink-800/40">
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
