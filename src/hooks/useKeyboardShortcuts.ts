import { useEffect, useCallback } from "react";

interface KeyboardShortcutsConfig {
  onCmdK?: () => void;
  onCmdF?: () => void;
  onEsc?: () => void;
  onCmdEnter?: () => void;
}

export function useKeyboardShortcuts(config: KeyboardShortcutsConfig) {
  const { onCmdK, onCmdF, onEsc, onCmdEnter } = config;

  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      const isCmd = e.metaKey || e.ctrlKey;
      const isShift = e.shiftKey;
      const isAlt = e.altKey;

      if (isCmd && !isShift && !isAlt && e.key === "k") {
        e.preventDefault();
        onCmdK?.();
      }

      if (isCmd && !isShift && !isAlt && e.key === "f") {
        e.preventDefault();
        onCmdF?.();
      }

      if (e.key === "Escape") {
        onEsc?.();
      }

      if (isCmd && e.key === "Enter") {
        e.preventDefault();
        onCmdEnter?.();
      }
    },
    [onCmdK, onCmdF, onEsc, onCmdEnter],
  );

  useEffect(() => {
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [handleKeyDown]);
}
