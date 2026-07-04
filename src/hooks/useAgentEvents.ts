import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useChatStore } from "@/stores/chatStore";
import { useAgentStore } from "@/stores/agentStore";
import type { AgentState } from "@/types/agent";

// Tauri 后端推送的事件载荷类型
interface TokenPayload {
  msgId: string;
  text: string;
}

interface StatusPayload {
  state: AgentState;
}

interface ErrorPayload {
  msgId: string;
  message: string;
}

interface DonePayload {
  msgId: string;
}

/**
 * 订阅 Rust Agent 推送到前端的所有事件，并把事件映射到 Zustand stores。
 * 挂载在 ChatView（根容器）即可，组件卸载时自动解绑。
 */
export function useAgentEvents() {
  const appendToken = useChatStore((s) => s.appendToken);
  const prepareAssistantMessage = useChatStore((s) => s.prepareAssistantMessage);
  const markComplete = useChatStore((s) => s.markComplete);
  const markError = useChatStore((s) => s.markError);

  const setAgentState = useAgentStore((s) => s.setState);
  const setAgentError = useAgentStore((s) => s.setError);

  useEffect(() => {
    const unlisteners: UnlistenFn[] = [];

    // 服务启动时探测一下 invoke 是否可用
    void (async () => {
      try {
        const handlers: Array<[string, (e: { payload: unknown }) => void]> = [
          [
            "agent:token",
            (e) => {
              const p = e.payload as TokenPayload;
              prepareAssistantMessage(p.msgId);
              appendToken(p.msgId, p.text);
            },
          ],
          [
            "agent:status",
            (e) => {
              const p = e.payload as StatusPayload;
              setAgentState(p.state);
            },
          ],
          [
            "agent:error",
            (e) => {
              const p = e.payload as ErrorPayload;
              setAgentError(p.message);
              markError(p.msgId, p.message);
            },
          ],
          [
            "agent:done",
            (e) => {
              const p = e.payload as DonePayload;
              markComplete(p.msgId);
            },
          ],
        ];

        for (const [name, handler] of handlers) {
          const unlisten = await listen<unknown>(name, handler);
          unlisteners.push(unlisten);
        }
      } catch (err) {
        // 非 Tauri 环境（vite dev 单独跑）下，invoke/listen 不可用，静默忽略
        // eslint-disable-next-line no-console
        console.warn("[useAgentEvents] Tauri APIs unavailable:", err);
      }
    })();

    return () => {
      unlisteners.forEach((u) => u());
    };
  }, [appendToken, prepareAssistantMessage, markComplete, markError, setAgentState, setAgentError]);
}

interface SendMessageArgs {
  text: string;
  assistantId: string;
}

/**
 * 发送一条用户消息给后端 Agent Loop。
 * 必须在 appendUserMessage 之后调用，因为需要把 assistant msgId 传给后端。
 */
export async function sendMessage({ text, assistantId }: SendMessageArgs) {
  // Tauri 2: invoke payload 是扁平对象，key 直接对应 Rust 命令参数名（camelCase）
  await invoke("send_message", { text, assistantId });
}