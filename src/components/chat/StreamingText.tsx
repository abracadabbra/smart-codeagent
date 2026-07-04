interface StreamingTextProps {
  text: string;
  streaming: boolean;
}

/**
 * 流式文本组件：逐字渲染 text，streaming 时末尾附加一个闪烁光标。
 * Phase 1 不做 markdown 渲染（PRD 标记后续 phase 引入 react-markdown），
 * 文本中的换行通过 whitespace-pre-wrap 保留。
 */
export function StreamingText({ text, streaming }: StreamingTextProps) {
  return (
    <div className="whitespace-pre-wrap break-words leading-relaxed">
      {text}
      {streaming && (
        <span
          className="ml-0.5 inline-block w-1.5 h-4 bg-ink-100 align-text-bottom animate-cursor-blink"
          aria-hidden="true"
        />
      )}
    </div>
  );
}