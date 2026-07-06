import { useMemo } from "react";
import { SessionItem } from "./SessionItem";
import { useSessionStore } from "@/stores/sessionStore";

export function SessionList() {
  const sessions = useSessionStore((s) => s.sessions);
  const activeSessionId = useSessionStore((s) => s.activeSessionId);
  const searchQuery = useSessionStore((s) => s.searchQuery);
  const setSearchQuery = useSessionStore((s) => s.setSearchQuery);
  const createSession = useSessionStore((s) => s.createSession);

  const filtered = useMemo(() => {
    if (!searchQuery.trim()) return sessions;
    const q = searchQuery.toLowerCase();
    return sessions.filter((s) => s.title.toLowerCase().includes(q));
  }, [sessions, searchQuery]);

  const pinned = filtered.filter((s) => s.pinned);
  const unpinned = filtered.filter((s) => !s.pinned);

  const handleNewSession = () => {
    void createSession();
  };

  return (
    <aside className="w-60 shrink-0 border-r border-ink-700 bg-ink-800/50 flex flex-col h-full">
      <div className="p-3 border-b border-ink-700">
        <div className="flex items-center justify-between mb-2">
          <div className="text-xs font-medium text-ink-200">Sessions</div>
          <button
            onClick={handleNewSession}
            className="p-1 rounded hover:bg-ink-700 text-ink-300 hover:text-ink-100 transition-colors"
            title="New session"
          >
            <svg className="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <line x1="12" y1="5" x2="12" y2="19" />
              <line x1="5" y1="12" x2="19" y2="12" />
            </svg>
          </button>
        </div>
        <div className="relative">
          <input
            type="text"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder="Search..."
            className="w-full bg-ink-900/60 border border-ink-600 rounded-md px-2 py-1 text-xs text-ink-100 placeholder-ink-500 focus:outline-none focus:border-blue-500/60 focus:ring-1 focus:ring-blue-500/30"
          />
          <svg
            className="absolute right-2 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-ink-500 pointer-events-none"
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

      <div className="flex-1 overflow-y-auto py-2 px-2 space-y-1">
        {pinned.length > 0 && (
          <>
            <div className="px-1 pt-1 pb-0.5 text-[10px] font-medium text-ink-500 uppercase tracking-wider">
              Pinned
            </div>
            {pinned.map((s) => (
              <SessionItem key={s.id} session={s} isActive={activeSessionId === s.id} />
            ))}
          </>
        )}

        {unpinned.length > 0 && (
          <>
            {pinned.length > 0 && (
              <div className="px-1 pt-2 pb-0.5 text-[10px] font-medium text-ink-500 uppercase tracking-wider">
                All
              </div>
            )}
            {unpinned.map((s) => (
              <SessionItem key={s.id} session={s} isActive={activeSessionId === s.id} />
            ))}
          </>
        )}

        {filtered.length === 0 && (
          <div className="px-2 py-8 text-center text-xs text-ink-500">
            {searchQuery ? "No matching sessions" : "No sessions yet"}
          </div>
        )}
      </div>
    </aside>
  );
}
