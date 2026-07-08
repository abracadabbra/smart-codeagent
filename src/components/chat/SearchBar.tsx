import { useEffect, useRef } from "react";
import { useChatStore } from "@/stores/chatStore";

export function SearchBar() {
  const searchOpen = useChatStore((s) => s.searchOpen);
  const query = useChatStore((s) => s.searchQuery);
  const results = useChatStore((s) => s.searchResults);
  const currentIndex = useChatStore((s) => s.searchCurrentIndex);
  const setSearchQuery = useChatStore((s) => s.setSearchQuery);
  const nextSearchResult = useChatStore((s) => s.nextSearchResult);
  const prevSearchResult = useChatStore((s) => s.prevSearchResult);
  const setSearchOpen = useChatStore((s) => s.setSearchOpen);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (searchOpen) {
      inputRef.current?.focus();
      inputRef.current?.select();
    }
  }, [searchOpen]);

  if (!searchOpen) return null;

  return (
    <div className="absolute top-4 left-1/2 -translate-x-1/2 z-20 flex items-center gap-2 px-3 py-2 rounded-xl bg-ink-900/95 border border-ink-700 shadow-xl backdrop-blur-sm">
      <svg className="w-4 h-4 text-ink-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
        <circle cx="11" cy="11" r="8" />
        <path d="m21 21-4.3-4.3" />
      </svg>

      <input
        ref={inputRef}
        type="text"
        value={query}
        onChange={(e) => setSearchQuery(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            if (e.shiftKey) {
              prevSearchResult();
            } else {
              nextSearchResult();
            }
          }
        }}
        placeholder="搜索消息..."
        className="w-48 bg-transparent text-sm text-ink-100 placeholder:text-ink-500 outline-none"
      />

      <span className="text-xs text-ink-400 tabular-nums min-w-[3.5rem] text-center">
        {results.length > 0 ? `${currentIndex + 1} / ${results.length}` : "0 / 0"}
      </span>

      <div className="flex items-center gap-1">
        <button
          onClick={prevSearchResult}
          disabled={results.length === 0}
          className="p-1 rounded-md text-ink-400 hover:bg-ink-800 hover:text-ink-200 disabled:opacity-30 disabled:cursor-not-allowed"
          title="上一个"
        >
          <svg className="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="m18 15-6-6-6 6" />
          </svg>
        </button>
        <button
          onClick={nextSearchResult}
          disabled={results.length === 0}
          className="p-1 rounded-md text-ink-400 hover:bg-ink-800 hover:text-ink-200 disabled:opacity-30 disabled:cursor-not-allowed"
          title="下一个"
        >
          <svg className="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="m6 9 6 6 6-6" />
          </svg>
        </button>
      </div>

      <button
        onClick={() => setSearchOpen(false)}
        className="p-1 rounded-md text-ink-400 hover:bg-ink-800 hover:text-ink-200"
        title="关闭 (Esc)"
      >
        <svg className="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
          <path d="M18 6 6 18" />
          <path d="m6 6 12 12" />
        </svg>
      </button>
    </div>
  );
}
