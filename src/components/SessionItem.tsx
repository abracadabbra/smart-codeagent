import { useSessionStore } from "@/stores/sessionStore";
import type { ConversationListItem } from "@/types/session";

interface SessionItemProps {
  session: ConversationListItem;
  isActive: boolean;
}

export function SessionItem({ session, isActive }: SessionItemProps) {
  const selectSession = useSessionStore((s) => s.selectSession);
  const generatingIds = useSessionStore((s) => s.generatingIds);
  const pendingApprovalIds = useSessionStore((s) => s.pendingApprovalIds);
  const togglePin = useSessionStore((s) => s.togglePin);
  const renameSession = useSessionStore((s) => s.renameSession);
  const deleteSession = useSessionStore((s) => s.deleteSession);

  const isGenerating = generatingIds.has(session.id);
  const hasPendingApproval = pendingApprovalIds.has(session.id);

  const handleClick = () => {
    selectSession(session.id);
  };

  const handlePin = (e: React.MouseEvent) => {
    e.stopPropagation();
    void togglePin(session.id);
  };

  const handleRename = (e: React.MouseEvent) => {
    e.stopPropagation();
    const newTitle = window.prompt("Rename session", session.title);
    if (newTitle && newTitle.trim() && newTitle !== session.title) {
      void renameSession(session.id, newTitle.trim());
    }
  };

  const handleDelete = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (window.confirm(`Delete "${session.title}"?`)) {
      void deleteSession(session.id);
    }
  };

  return (
    <div
      onClick={handleClick}
      className={[
        "group relative rounded-md p-2.5 cursor-pointer transition-colors",
        "border border-transparent",
        isActive
          ? "bg-ink-700/80 border-ink-500 text-ink-50"
          : "hover:bg-ink-700/40 text-ink-200",
      ].join(" ")}
    >
      <div className="flex items-start gap-2">
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-1.5">
            <div
              className={[
                "text-xs font-medium truncate",
                isActive ? "text-ink-50" : "text-ink-200",
              ].join(" ")}
              title={session.title}
            >
              {session.title}
            </div>
            {session.pinned && (
              <svg
                className="w-3 h-3 text-amber-400/80 shrink-0"
                viewBox="0 0 24 24"
                fill="currentColor"
              >
                <path d="M16 12V4h1V2H7v2h1v8l-2 2v2h5.2v6h1.6v-6H18v-2l-2-2z" />
              </svg>
            )}
          </div>
          <div
            className="text-[11px] text-ink-400 truncate mt-0.5"
            title={session.preview}
          >
            {session.preview || "No messages yet"}
          </div>
        </div>

        <div className="flex items-center gap-1 shrink-0">
          {isGenerating && (
            <div className="w-3 h-3 flex items-center justify-center">
              <div className="w-2.5 h-2.5 rounded-full border border-blue-400/60 border-t-transparent animate-spin" />
            </div>
          )}
          {hasPendingApproval && !isGenerating && (
            <div className="w-2 h-2 rounded-full bg-red-500 shrink-0" />
          )}
        </div>
      </div>

      <div
        className={[
          "absolute top-1 right-1 flex gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity",
        ].join(" ")}
      >
        <button
          onClick={handlePin}
          className="p-1 rounded hover:bg-ink-600/70 text-ink-400 hover:text-ink-200"
          title={session.pinned ? "Unpin" : "Pin"}
        >
          <svg className="w-3.5 h-3.5" viewBox="0 0 24 24" fill="currentColor">
            <path d="M16 12V4h1V2H7v2h1v8l-2 2v2h5.2v6h1.6v-6H18v-2l-2-2z" />
          </svg>
        </button>
        <button
          onClick={handleRename}
          className="p-1 rounded hover:bg-ink-600/70 text-ink-400 hover:text-ink-200"
          title="Rename"
        >
          <svg className="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="M11 4H4a2 2 0 00-2 2v14a2 2 0 002 2h14a2 2 0 002-2v-7" />
            <path d="M18.5 2.5a2.121 2.121 0 013 3L12 15l-4 1 1-4 9.5-9.5z" />
          </svg>
        </button>
        <button
          onClick={handleDelete}
          className="p-1 rounded hover:bg-red-900/50 text-ink-400 hover:text-red-400"
          title="Delete"
        >
          <svg className="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <polyline points="3 6 5 6 21 6" />
            <path d="M19 6l-2 14a2 2 0 01-2 2H9a2 2 0 01-2-2L5 6" />
            <path d="M10 11v6M14 11v6" />
            <path d="M9 6V4a2 2 0 012-2h2a2 2 0 012 2v2" />
          </svg>
        </button>
      </div>
    </div>
  );
}
