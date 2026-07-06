import { useCallback } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

/**
 * macOS Overlay 标题栏。
 *
 * Tauri v2 的 data-tauri-drag-region 在 Overlay 模式下偶尔失效，
 * 这里通过 onMouseDown 手动调用 startDragging() 作为兜底方案。
 * 按钮区域标记 data-tauri-no-drag-region，避免拖拽吞掉点击。
 */
export function TitleBar() {
  const startDrag = useCallback(async () => {
    try {
      await getCurrentWindow().startDragging();
    } catch {
      // ignore
    }
  }, []);

  return (
    <header
      onMouseDown={startDrag}
      className="absolute top-0 left-0 right-0 h-11 z-50 flex items-center justify-between px-4 titlebar-safe"
    >
      {/* 左侧留白区域 — 可拖拽 */}
      <div className="flex-1 h-full" onMouseDown={startDrag} />

      {/* 右侧可操作区 */}
      <div className="flex items-center gap-2 h-full" data-tauri-no-drag-region>
        <button
          type="button"
          className="flex items-center gap-1.5 text-xs text-ink-400 hover:text-ink-200 transition-colors px-2 py-1 rounded-md hover:bg-ink-800/50"
        >
          <span>打开编辑器</span>
          <svg className="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" />
            <polyline points="15 3 21 3 21 9" />
            <line x1="10" y1="14" x2="21" y2="3" />
          </svg>
        </button>
      </div>
    </header>
  );
}
