import { StreamingText } from "./StreamingText";
import type { Message } from "@/types/message";

interface MessageBubbleProps {
  message: Message;
}

/**
 * 消息气泡：
 * - 用户：右侧对齐，蓝色背景
 * - 助手：左侧对齐，深色背景，状态决定光标
 */
export function MessageBubble({ message }: MessageBubbleProps) {
  const isUser = message.role === "user";

  return (
    <div className={`flex ${isUser ? "justify-end" : "justify-start"} my-3`}>
      <div
        className={[
          "max-w-[80%] rounded-2xl px-4 py-3",
          isUser
            ? "bg-blue-600/25 text-ink-50 rounded-tr-sm"
            : "bg-ink-700 text-ink-100 rounded-tl-sm",
          message.status === "error" ? "ring-1 ring-red-500/50" : "",
        ].join(" ")}
      >
        {isUser ? (
          <div className="whitespace-pre-wrap break-words">{message.content}</div>
        ) : (
          <StreamingText
            text={message.content}
            streaming={message.status === "streaming"}
          />
        )}

        {message.status === "error" && message.error && (
          <div className="mt-2 text-xs text-red-300/80 border-t border-red-500/30 pt-2">
            {message.error}
          </div>
        )}
      </div>
    </div>
  );
}