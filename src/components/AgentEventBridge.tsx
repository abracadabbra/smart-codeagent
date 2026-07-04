import { useAgentEvents } from "@/hooks/useAgentEvents";

/**
 * 隐性订阅组件：把 useAgentEvents 的副作用挂在这里，
 * 避免 ChatView 自己关心 Tauri 事件订阅的生命周期。
 * 在 App 中渲染一次即可。
 */
export function AgentEventBridge() {
  useAgentEvents();
  return null;
}