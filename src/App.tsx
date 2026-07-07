import { useEffect } from "react";
import { ChatView } from "@/components/chat/ChatView";
import { AgentEventBridge } from "@/components/AgentEventBridge";
import { PreviewPane } from "@/components/PreviewPane";
import { ApprovalDialog } from "@/components/chat/ApprovalDialog";
import { StatusBar } from "@/components/StatusBar";
import { SessionList } from "@/components/SessionList";
import { TitleBar } from "@/components/TitleBar";
import { SettingsPanel } from "@/components/SettingsPanel";
import { initSessionStore } from "@/stores/sessionStore";
import { useChatStore } from "@/stores/chatStore";
import { useAgentStore } from "@/stores/agentStore";
import { useSessionStore } from "@/stores/sessionStore";
import { useMcpStore } from "@/stores/mcpStore";

export function App() {
  const activeSessionId = useSessionStore((s) => s.activeSessionId);
  const setActiveChatConversation = useChatStore((s) => s.setActiveConversation);
  const setActiveAgentConversation = useAgentStore((s) => s.setActiveConversation);
  const showSettings = useMcpStore((s) => s.showSettings);
  const setShowSettings = useMcpStore((s) => s.setShowSettings);

  useEffect(() => {
    initSessionStore();
  }, []);

  useEffect(() => {
    setActiveChatConversation(activeSessionId);
    setActiveAgentConversation(activeSessionId);
  }, [activeSessionId, setActiveChatConversation, setActiveAgentConversation]);

  return (
    <div className="flex flex-col h-screen bg-ink-950 text-ink-100 relative">
      <AgentEventBridge />
      <TitleBar />

      <main className="flex-1 flex overflow-hidden pt-11">
        <SessionList />
        <section className="flex-1 min-w-0 bg-ink-950 relative">
          <ChatView />
        </section>
        <PreviewPane />
      </main>

      <StatusBar />
      <ApprovalDialog />
      {showSettings && <SettingsPanel onClose={() => setShowSettings(false)} />}
    </div>
  );
}
