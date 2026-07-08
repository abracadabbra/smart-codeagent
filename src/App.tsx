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
import { useKeyboardShortcuts } from "@/hooks/useKeyboardShortcuts";
import { checkUpdate, downloadAndInstall } from "@/lib/updater";

export function App() {
  const activeSessionId = useSessionStore((s) => s.activeSessionId);
  const setActiveChatConversation = useChatStore((s) => s.setActiveConversation);
  const setActiveAgentConversation = useAgentStore((s) => s.setActiveConversation);
  const showSettings = useMcpStore((s) => s.showSettings);
  const setShowSettings = useMcpStore((s) => s.setShowSettings);
  const createSession = useSessionStore((s) => s.createSession);
  const searchOpen = useChatStore((s) => s.searchOpen);
  const toggleSearch = useChatStore((s) => s.toggleSearch);
  const setSearchOpen = useChatStore((s) => s.setSearchOpen);

  useEffect(() => {
    initSessionStore();
  }, []);

  // Phase 5.7: 启动 5s 后静默检查更新，发现新版本时弹窗询问是否下载安装。
  useEffect(() => {
    const timer = setTimeout(async () => {
      try {
        const info = await checkUpdate();
        if (info) {
          const ok = window.confirm(
            `发现新版本 v${info.version}，是否下载并安装？\n\n${info.body || ""}`
          );
          if (ok) {
            await downloadAndInstall();
          }
        }
      } catch {
        // 静默失败，不打断用户。
      }
    }, 5000);
    return () => clearTimeout(timer);
  }, []);

  useEffect(() => {
    setActiveChatConversation(activeSessionId);
    setActiveAgentConversation(activeSessionId);
  }, [activeSessionId, setActiveChatConversation, setActiveAgentConversation]);

  useKeyboardShortcuts({
    onCmdK: () => {
      createSession();
    },
    onCmdF: () => {
      toggleSearch();
    },
    onEsc: () => {
      if (showSettings) {
        setShowSettings(false);
      } else if (searchOpen) {
        setSearchOpen(false);
      }
    },
  });

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
