import { ChatView } from "@/components/chat/ChatView";
import { AgentEventBridge } from "@/components/AgentEventBridge";

/**
 * 三栏布局（左 Session 占位 / 中 对话 / 右 文件预览占位） + 顶部标题栏。
 * Tauri 桌面窗口不需要传统浏览器 chrome，由 titleBarStyle 控制（见 tauri.conf.json）。
 */
export function App() {
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
        {/* 左侧 Session 列表占位 */}
        <aside className="w-60 shrink-0 border-r border-ink-700 bg-ink-800/50 p-3 text-xs text-ink-300">
          <div className="font-medium text-ink-200 mb-2">Session</div>
          <div className="rounded-md border border-dashed border-ink-600 p-3 text-center">
            占位 — Phase 3 添加多 Session
          </div>
        </aside>

        {/* 中栏 */}
        <section className="flex-1 min-w-0">
          <ChatView />
        </section>

        {/* 右侧文件预览 / 工具结果占位 */}
        <aside className="w-80 shrink-0 border-l border-ink-700 bg-ink-800/30 p-3 text-xs text-ink-300">
          <div className="font-medium text-ink-200 mb-2">Preview</div>
          <div className="rounded-md border border-dashed border-ink-600 p-3 text-center">
            占位 — Phase 2 添加文件预览 / 工具结果
          </div>
        </aside>
      </main>
    </div>
  );
}