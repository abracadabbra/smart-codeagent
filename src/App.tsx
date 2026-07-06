import { useEffect } from "react";
import { ChatView } from "@/components/chat/ChatView";
import { AgentEventBridge } from "@/components/AgentEventBridge";
import { PreviewPane } from "@/components/PreviewPane";
import { ApprovalDialog } from "@/components/chat/ApprovalDialog";
import { StatusBar } from "@/components/StatusBar";
import { SessionList } from "@/components/SessionList";
import { initSessionStore } from "@/stores/sessionStore";
import { useChatStore } from "@/stores/chatStore";
import { useAgentStore } from "@/stores/agentStore";
import { useSessionStore } from "@/stores/sessionStore";

/**
 * 三栏布局（左 Session 列表 / 中 对话 / 右 工具预览） + 顶部标题栏 + 底部 MCP 状态栏。
 * ApprovalDialog 由 store 中的 approvalRequest 触发，挂在 body 顶层 modal。
 *
 * Phase 3.2: 左侧 SessionList 替换占位；初始化时 load session 列表 + 自动选中第一个；
 * 切换 session 时 chatStore/agentStore 同步切换状态。
 */
export function App() {
  const activeSessionId = useSessionStore((s) => s.activeSessionId);
  const setActiveChatConversation = useChatStore((s) => s.setActiveConversation);
  const setActiveAgentConversation = useAgentStore((s) => s.setActiveConversation);

  useEffect(() => {
    initSessionStore();
  }, []);

  // 当 active session 切换时，同步切换 chatStore/agentStore 的 active conversation
  useEffect(() => {
    setActiveChatConversation(activeSessionId);
    setActiveAgentConversation(activeSessionId);
  }, [activeSessionId, setActiveChatConversation, setActiveAgentConversation]);

  return (
    <div className="flex flex-col h-screen bg-ink-900 text-ink-100">
      {/* 副作用：一次性挂载 Tauri 事件订阅 */}
      <AgentEventBridge />

      {/* 顶部标题栏 —— Phase 1 仅做品牌名 */}
      <header className="h-10 border-b border-ink-700 flex items-center px-4 text-sm font-medium">
        Smart CodeAgent
      </header>

      {/* 三栏主体 */}
      <main className="flex-1 flex overflow-hidden">
        {/* 左侧 Session 列表 */}
        <SessionList />

        {/* 中栏 */}
        <section className="flex-1 min-w-0">
          <ChatView />
        </section>

        {/* 右侧 工具调用预览 */}
        <PreviewPane />
      </main>

      {/* 底部 MCP 状态栏 — hover 展开完整 server 列表 */}
      <StatusBar />

      {/* 全局 modal：approval_request 触发时显示 */}
      <ApprovalDialog />
    </div>
  );
}
