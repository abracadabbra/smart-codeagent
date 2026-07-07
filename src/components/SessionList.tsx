import { useMemo } from "react";
import { SessionItem } from "./SessionItem";
import { useSessionStore } from "@/stores/sessionStore";
import { useMcpStore } from "@/stores/mcpStore";

/**
 * 左侧项目/Quest 风格会话列表。
 * - 顶部 "+ 创建 Quest" 大按钮
 * - 简单按日期分组：Today / Yesterday / Earlier
 * - 每项左侧图标 + 标题 + 右侧相对时间
 */
function formatRelativeDay(timestamp: number): string {
  const now = new Date();
  const d = new Date(timestamp);
  const diffDays = Math.floor(
    (new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime() -
      new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime()) /
      (1000 * 60 * 60 * 24),
  );
  if (diffDays === 0) return "今天";
  if (diffDays === 1) return "昨天";
  if (diffDays < 7) return `${diffDays} 天前`;
  return `${Math.floor(diffDays / 7)} 周前`;
}

export function SessionList() {
  const sessions = useSessionStore((s) => s.sessions);
  const activeSessionId = useSessionStore((s) => s.activeSessionId);
  const searchQuery = useSessionStore((s) => s.searchQuery);
  const setSearchQuery = useSessionStore((s) => s.setSearchQuery);
  const createSession = useSessionStore((s) => s.createSession);
  const setShowSettings = useMcpStore((s) => s.setShowSettings);

  const filtered = useMemo(() => {
    if (!searchQuery.trim()) return sessions;
    const q = searchQuery.toLowerCase();
    return sessions.filter((s) => s.title.toLowerCase().includes(q));
  }, [sessions, searchQuery]);

  const pinned = filtered.filter((s) => s.pinned);
  const unpinned = filtered.filter((s) => !s.pinned);

  const today = unpinned.filter((s) => formatRelativeDay(s.updatedAt) === "今天");
  const yesterday = unpinned.filter((s) => formatRelativeDay(s.updatedAt) === "昨天");
  const earlier = unpinned.filter((s) => {
    const label = formatRelativeDay(s.updatedAt);
    return label !== "今天" && label !== "昨天";
  });

  return (
    <aside className="w-64 shrink-0 border-r border-ink-900 bg-ink-950 flex flex-col h-full">
      {/* 创建按钮 */}
      <div className="px-3 pt-3 pb-3">
        <button
          onClick={() => void createSession()}
          className="w-full flex items-center justify-between px-3 py-2 rounded-lg bg-surface-raised hover:bg-surface-hover border border-ink-800/60 transition-colors text-sm text-ink-100"
        >
          <span className="flex items-center gap-2">
            <svg className="w-4 h-4 text-ink-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <line x1="12" y1="5" x2="12" y2="19" />
              <line x1="5" y1="12" x2="19" y2="12" />
            </svg>
            创建 Quest
          </span>
          <span className="text-[10px] text-ink-500 border border-ink-700 rounded px-1">⌘ N</span>
        </button>
      </div>

      {/* 搜索 */}
      <div className="px-3 pb-2">
        <div className="relative">
          <input
            type="text"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder="Search quests..."
            className="w-full bg-ink-950 border border-ink-800 rounded-lg pl-8 pr-2 py-1.5 text-xs text-ink-100 placeholder-ink-600 focus:outline-none focus:border-ink-600 focus:ring-1 focus:ring-ink-700"
          />
          <svg
            className="absolute left-2.5 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-ink-600 pointer-events-none"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
          >
            <circle cx="11" cy="11" r="8" />
            <line x1="21" y1="21" x2="16.65" y2="16.65" />
          </svg>
        </div>
      </div>

      {/* 列表 */}
      <div className="flex-1 overflow-y-auto py-1 px-2 space-y-3">
        {pinned.length > 0 && (
          <div className="space-y-0.5">
            <div className="px-2 pt-1 pb-1 text-[10px] font-semibold text-ink-500 uppercase tracking-wider">
              Pinned
            </div>
            {pinned.map((s) => (
              <SessionItem key={s.id} session={s} isActive={activeSessionId === s.id} />
            ))}
          </div>
        )}

        {today.length > 0 && (
          <div className="space-y-0.5">
            <div className="px-2 pt-1 pb-1 text-[10px] font-semibold text-ink-500 uppercase tracking-wider">
              今天
            </div>
            {today.map((s) => (
              <SessionItem key={s.id} session={s} isActive={activeSessionId === s.id} />
            ))}
          </div>
        )}

        {yesterday.length > 0 && (
          <div className="space-y-0.5">
            <div className="px-2 pt-1 pb-1 text-[10px] font-semibold text-ink-500 uppercase tracking-wider">
              昨天
            </div>
            {yesterday.map((s) => (
              <SessionItem key={s.id} session={s} isActive={activeSessionId === s.id} />
            ))}
          </div>
        )}

        {earlier.length > 0 && (
          <div className="space-y-0.5">
            <div className="px-2 pt-1 pb-1 text-[10px] font-semibold text-ink-500 uppercase tracking-wider">
              更早
            </div>
            {earlier.map((s) => (
              <SessionItem key={s.id} session={s} isActive={activeSessionId === s.id} />
            ))}
          </div>
        )}

        {filtered.length === 0 && (
          <div className="px-2 py-8 text-center text-xs text-ink-500">
            {searchQuery ? "No matching quests" : "No quests yet"}
          </div>
        )}
      </div>

      {/* 底部用户区 */}
      <div className="p-3 border-t border-ink-900">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2 min-w-0">
            <div className="w-7 h-7 rounded-full bg-gradient-to-br from-brand-500 to-purple-600 flex items-center justify-center text-[10px] font-bold text-white shrink-0">
              涛
            </div>
            <div className="min-w-0">
              <div className="text-xs font-medium text-ink-200 truncate">涛 沈</div>
              <div className="text-[10px] text-ink-500 truncate">Teams</div>
            </div>
          </div>
          <div className="flex items-center gap-0.5">
            <button className="p-1.5 rounded-md text-ink-500 hover:bg-ink-800 hover:text-ink-200 transition-colors" title="Help">
              <svg className="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <circle cx="12" cy="12" r="10" />
                <path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3" />
                <line x1="12" y1="17" x2="12.01" y2="17" />
              </svg>
            </button>
            <button className="p-1.5 rounded-md text-ink-500 hover:bg-ink-800 hover:text-ink-200 transition-colors" title="Settings" onClick={() => setShowSettings(true)}>
              <svg className="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <circle cx="12" cy="12" r="3" />
                <path d="M12 1v6m0 6v6m4.22-10.22l4.24-4.24M6.34 6.34L2.1 2.1m17.9 10.9h-6m-6 0H1.9m17.8 0h.01M16.24 17.66l4.24 4.24M6.34 17.66l-4.24 4.24" />
              </svg>
            </button>
          </div>
        </div>
      </div>
    </aside>
  );
}
