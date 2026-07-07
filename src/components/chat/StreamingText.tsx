import { useState, useRef, useEffect } from "react";
import { MarkdownMessage } from "./MarkdownMessage";

interface StreamingTextProps {
  text: string;
  streaming: boolean;
}

/**
 * 流式文本组件。
 *
 * 关键策略：
 * - streaming 时**不解析 Markdown**，直接按纯文本预格式化渲染。因为流式过程中
 *   代码块围栏（```）可能尚未闭合，此时解析 Markdown 会把代码块内容当成普通
 *   段落拍平，导致排版混乱（用户反馈的"乱七八糟"）。
 * - streaming 完成后再用 MarkdownMessage 正常渲染 Markdown。
 *
 * 性能优化：
 * - 渲染节流：流式输出时每 RENDER_INTERVAL_MS ms 才更新一次屏幕，避免每 token
 *   重渲染阻塞主线程。
 */
const RENDER_INTERVAL_MS = 150;

export function StreamingText({ text, streaming }: StreamingTextProps) {
  const [renderedText, setRenderedText] = useState(text);
  const lastRenderTimeRef = useRef(0);
  const rafRef = useRef<number | null>(null);

  useEffect(() => {
    if (!streaming) {
      setRenderedText(text);
      return;
    }

    const now = Date.now();
    const elapsed = now - lastRenderTimeRef.current;

    if (elapsed >= RENDER_INTERVAL_MS) {
      lastRenderTimeRef.current = now;
      setRenderedText(text);
    } else if (rafRef.current === null) {
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

  if (streaming) {
    return (
      <div className="relative whitespace-pre-wrap font-mono text-[13px] leading-[1.6] text-ink-100">
        {renderedText}
        <span
          className="inline-block w-1.5 h-3.5 bg-ink-100 align-text-bottom animate-cursor-blink ml-0.5 rounded-sm"
          aria-hidden="true"
        />
      </div>
    );
  }

  return <MarkdownMessage content={renderedText} />;
}
