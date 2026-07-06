import { useEffect, useRef, useState } from "react";
import { useSessionStore } from "@/stores/sessionStore";
import type { ConversationListItem } from "@/types/session";

interface SessionItemProps {
  session: ConversationListItem;
  isActive: boolean;
}

function formatRelativeTime(timestamp: number): string {
  const now = Date.now();
  const diff = now - timestamp;
  const minutes = Math.floor(diff / (1000 * 60));
  const hours = Math.floor(diff / (1000 * 60 * 60));
  const days = Math.floor(diff / (1000 * 60 * 60 * 24));
  if (minutes < 1) return "刚刚";
  if (minutes < 60) return `${minutes} 分钟前`;
  if (hours < 24) return `${hours} 小时前`;
  if (days < 7) return `${days} 天前`;
  return `${Math.floor(days / 7)} 周前`;
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

  const [isRenaming, setIsRenaming] = useState(false);
  const [draftTitle, setDraftTitle] = useState(session.title);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (isRenaming && inputRef.current) {
      inputRef.current.focus();
      inputRef.current.select();
    }
  }, [isRenaming]);

  const handleClick = () => {
    if (isRenaming) return;
    selectSession(session.id);
  };

  const handlePin = (e: React.MouseEvent) => {
    e.stopPropagation();
    void togglePin(session.id);
  };

  const handleRename = (e: React.MouseEvent) => {
    e.stopPropagation();
    setDraftTitle(session.title);
    setIsRenaming(true);
  };

  const commitRename = async () => {
    const newTitle = draftTitle.trim();
    if (newTitle && newTitle !== session.title) {
      try {
        await renameSession(session.id, newTitle);
      } catch {
        setDraftTitle(session.title);
      }
    } else {
      setDraftTitle(session.title);
    }
    setIsRenaming(false);
  };

  const handleRenameKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    e.stopPropagation();
    if (e.key === "Enter") {
      e.preventDefault();
      void commitRename();
    } else if (e.key === "Escape") {
      e.preventDefault();
      setDraftTitle(session.title);
      setIsRenaming(false);
    }
  };

  const handleRenameBlur = () => {
    void commitRename();
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
        "group relative rounded-lg px-2 py-2 cursor-pointer transition-colors",
        "border border-transparent",
        isActive
          ? "bg-ink-800/80 border-ink-700 text-ink-50"
          : "hover:bg-ink-800/50 text-ink-200",
      ].join(" ")}
    >
      <div className="flex items-center gap-2.5">
        <div className="shrink-0 w-5 h-5 rounded flex items-center justify-center bg-ink-800 text-ink-400">
          <svg className="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
            <polyline points="14 2 14 8 20 8" />
            <line x1="16" y1="13" x2="8" y2="13" />
            <line x1="16" y1="17" x2="8" y2="17" />
            <polyline points="10 9 9 9 8 9" />
          </svg>
        </div>

        <div className="flex-1 min-w-0">
          {isRenaming ? (
            <input
              ref={inputRef}
              type="text"
              value={draftTitle}
              onChange={(e) => setDraftTitle(e.target.value)}
              onKeyDown={handleRenameKeyDown}
              onBlur={handleRenameBlur}
              onClick={(e) => e.stopPropagation()}
              className="w-full bg-ink-950 border border-brand-500/60 rounded px-1.5 py-0.5 text-xs text-ink-50 focus:outline-none focus:ring-1 focus:ring-brand-500/40"
            />
          ) : (
            <div
              className={[
                "text-xs truncate",
                isActive ? "text-ink-50 font-medium" : "text-ink-200",
              ].join(" ")}
              title={session.title}
            >
              {session.title}
            </div>
          )}
        </div>

        <div className="flex items-center gap-1 shrink-0">
          {isGenerating && (
            <div className="w-3 h-3 flex items-center justify-center">
              <div className="w-2 h-2 rounded-full border border-brand-400/60 border-t-transparent animate-spin" />
            </div>
          )}
          {hasPendingApproval && !isGenerating && (
            <div className="w-1.5 h-1.5 rounded-full bg-red-500" />
          )}
          {!isRenaming && (
            <span className="text-[10px] text-ink-600">
              {formatRelativeTime(session.updatedAt)}
            </span>
          )}
        </div>
      </div>

      {!isRenaming && (
        <div className="absolute top-1 right-1 hidden group-hover:flex gap-0.5 bg-ink-900/90 rounded-md p-0.5">
          <button
            onClick={handlePin}
            className="p-1 rounded hover:bg-ink-700 text-ink-500 hover:text-ink-200"
            title={session.pinned ? "Unpin" : "Pin"}
          >
            <svg className="w-3 h-3" viewBox="0 0 24 24" fill="currentColor">
              <path d="M16 12V4h1V2H7v2h1v8l-2 2v2h5.2v6h1.6v-6H18v-2l-2-2z" />
            </svg>
          </button>
          <button
            onClick={handleRename}
            className="p-1 rounded hover:bg-ink-700 text-ink-500 hover:text-ink-200"
            title="Rename"
          >
            <svg className="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <path d="M11 4H4a2 2 0 00-2 2v14a2 2 0 002 2h14a2 2 0 002-2v-7" />
              <path d="M18.5 2.5a2.121 2.121 0 013 3L12 15l-4 1 1-4 9.5-9.5z" />
            </svg>
          </button>
          <button
            onClick={handleDelete}
            className="p-1 rounded hover:bg-red-900/50 text-ink-500 hover:text-red-400"
            title="Delete"
          >
            <svg className="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <polyline points="3 6 5 6 21 6" />
              <path d="M19 6l-2 14a2 2 0 01-2 2H9a2 2 0 01-2-2L5 6" />
              <path d="M10 11v6M14 11v6" />
              <path d="M9 6V4a2 2 0 012-2h2a2 2 0 012 2v2" />
            </svg>
          </button>
        </div>
      )}
    </div>
  );
}
