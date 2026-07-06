import { useMemo, useRef, useState, useEffect } from "react";
import { MarkdownMessage } from "./MarkdownMessage";

interface StreamingTextProps {
  text: string;
  streaming: boolean;
}

/**
 * 流式文本组件：支持 Markdown 渲染。
 * streaming 时在末尾附加闪烁光标。
 *
 * 性能优化：
 * 1. 代码围栏保护：未闭合的 ``` 数量为奇数时，只渲染到最后一个 ``` 之前，避免吞掉后续文本
 * 2. 渲染节流：流式输出时不每 token 都解析 Markdown（会导致主线程阻塞、UI 卡顿），
 *    而是每 RENDER_INTERVAL_MS ms 才更新一次渲染内容
 */
const RENDER_INTERVAL_MS = 150;

export function StreamingText({ text, streaming }: StreamingTextProps) {
  // 节流：流式输出时每隔一段时间才更新渲染文本
  const [renderedText, setRenderedText] = useState(text);
  const lastRenderTimeRef = useRef(0);
  const rafRef = useRef<number | null>(null);

  useEffect(() => {
    if (!streaming) {
      // 非流式状态，直接同步
      setRenderedText(text);
      return;
    }

    const now = Date.now();
    const elapsed = now - lastRenderTimeRef.current;

    if (elapsed >= RENDER_INTERVAL_MS) {
      // 已超过节流间隔，立即更新
      lastRenderTimeRef.current = now;
      setRenderedText(text);
    } else if (rafRef.current === null) {
      // 安排下一次更新
      const delay = RENDER_INTERVAL_MS - elapsed;
      rafRef.current = window.setTimeout(() => {
        lastRenderTimeRef.current = Date.now();
        setRenderedText(text);
        rafRef.current = null;
      }, delay);
    }

    return () => {
      if (rafRef.current !== null) {
        clearTimeout(rafRef.current);
        rafRef.current = null;
      }
    };
  }, [text, streaming]);

  // 最终完成时确保渲染全部内容
  useEffect(() => {
    if (!streaming) {
      setRenderedText(text);
    }
  }, [streaming, text]);

  const displayText = useMemo(() => {
    if (!streaming) return renderedText;
    const fences = Array.from(renderedText.matchAll(/```/g));
    if (fences.length % 2 === 1) {
      const lastFenceIndex = fences[fences.length - 1].index ?? renderedText.length;
      return renderedText.slice(0, lastFenceIndex);
    }
    return renderedText;
  }, [renderedText, streaming]);

  return (
    <div className="relative">
      <MarkdownMessage content={displayText} />
      {streaming && (
        <span
          className="inline-block w-1.5 h-3.5 bg-ink-100 align-text-bottom animate-cursor-blink ml-0.5 rounded-sm"
          aria-hidden="true"
        />
      )}
    </div>
  );
}
