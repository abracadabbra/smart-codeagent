import {
  useEffect,
  useRef,
  useState,
  useCallback,
  forwardRef,
  useImperativeHandle,
} from "react";
import { MessageBubble } from "./MessageBubble";
import type { Message } from "@/types/message";

export interface VirtualMessageListHandle {
  /** 滚动到底部 */
  scrollToBottom: (behavior?: ScrollBehavior) => void;
  /** 滚动到指定消息 */
  scrollToIndex: (index: number) => void;
}

interface VirtualMessageListProps {
  messages: Message[];
  scrollToIndex: number | null;
}

const OVERSCAN = 6;
const DEFAULT_ESTIMATE_HEIGHT = 80;

/**
 * 估算消息高度（像素）。
 *
 * 基于文本长度做简单线性估计：
 * - 每 120 字符约一行 24px
 * - 代码块、工具结果等会显著增加高度，通过最小高度兜底
 */
function estimateHeight(message: Message): number {
  const text = message.content || "";
  const lines = Math.max(1, Math.ceil(text.length / 120));
  return Math.max(DEFAULT_ESTIMATE_HEIGHT, lines * 24 + 48);
}

/**
 * 消息虚拟列表（无外部依赖）。
 *
 * 解决几百条消息后前端卡顿的问题：
 * - 仅渲染视口上下 overscan 范围内的消息
 * - 移出视口的消息替换为等高校位占位 div
 * - 使用 ResizeObserver 持续测量真实消息高度并缓存
 * - 占位 div 仍保留 data-message-index，保证搜索滚动定位可用
 *
 * 与原生 content-visibility 相比，本组件从 React 层面减少渲染节点数，
 * 对搜索高亮、流式输出高度变化等动态场景更可控。
 */
export const VirtualMessageList = forwardRef<
  VirtualMessageListHandle,
  VirtualMessageListProps
>(function VirtualMessageList({ messages, scrollToIndex }, ref) {
  const containerRef = useRef<HTMLDivElement>(null);
  const itemRefs = useRef<Map<number, HTMLDivElement>>(new Map());
  const heightsRef = useRef<Map<number, number>>(new Map());

  // visibleSet 保存当前应渲染完整 MessageBubble 的消息索引
  const [visibleSet, setVisibleSet] = useState<Set<number>>(() => new Set());

  useImperativeHandle(ref, () => ({
    scrollToBottom: (behavior = "auto") => {
      const container = containerRef.current;
      if (!container) return;
      container.scrollTo({ top: container.scrollHeight, behavior });
    },
    scrollToIndex: (index: number) => {
      const el = itemRefs.current.get(index);
      if (!el) return;
      el.scrollIntoView({ behavior: "smooth", block: "center" });
    },
  }));

  // 滚动到指定消息：占位 div 进入视口后会触发 IntersectionObserver，
  // 自动渲染真实消息并滚动到可视区域中心
  useEffect(() => {
    if (scrollToIndex == null) return;
    const el = itemRefs.current.get(scrollToIndex);
    if (!el) return;
    el.scrollIntoView({ behavior: "smooth", block: "center" });
  }, [scrollToIndex]);

  // IntersectionObserver：监听所有占位/真实项，维护可见索引集合
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const observer = new IntersectionObserver(
      (entries) => {
        const newVisible = new Set<number>();
        entries.forEach((entry) => {
          if (entry.isIntersecting) {
            const index = Number(entry.target.getAttribute("data-index"));
            if (!Number.isNaN(index)) {
              newVisible.add(index);
            }
          }
        });

        if (newVisible.size === 0) return;

        const min = Math.min(...newVisible);
        const max = Math.max(...newVisible);
        const start = Math.max(0, min - OVERSCAN);
        const end = Math.min(messages.length - 1, max + OVERSCAN);

        setVisibleSet((prev) => {
          // 直接设置为当前视口范围 + overscan，移除范围外的已渲染项
          const next = new Set<number>();
          for (let i = start; i <= end; i++) {
            next.add(i);
          }
          if (next.size === prev.size) {
            let same = true;
            for (const idx of next) {
              if (!prev.has(idx)) {
                same = false;
                break;
              }
            }
            if (same) return prev;
          }
          return next;
        });
      },
      {
        root: container,
        threshold: 0,
        rootMargin: `${OVERSCAN * DEFAULT_ESTIMATE_HEIGHT}px 0px`,
      },
    );

    itemRefs.current.forEach((el) => observer.observe(el));
    return () => observer.disconnect();
  }, [messages.length]);

  // ResizeObserver：测量真实消息高度并缓存，用于占位 div 的准确高度
  useEffect(() => {
    const container = containerRef.current;
    if (!container || visibleSet.size === 0) return;

    const resizeObserver = new ResizeObserver((entries) => {
      entries.forEach((entry) => {
        const index = Number(entry.target.getAttribute("data-index"));
        if (Number.isNaN(index)) return;
        const newHeight = entry.contentRect.height;
        heightsRef.current.set(index, newHeight);
      });
      // 注意：这里不触发 re-render。
      // heightsRef 仅用于隐藏项的占位高度；当前可见项由真实内容决定高度。
      // 若隐藏期间内容发生变化，下次滚动回该消息时会重新测量。
    });

    visibleSet.forEach((index) => {
      const el = itemRefs.current.get(index);
      if (el) resizeObserver.observe(el);
    });

    return () => resizeObserver.disconnect();
  }, [visibleSet]);

  const setItemRef = useCallback((el: HTMLDivElement | null, index: number) => {
    if (el) {
      itemRefs.current.set(index, el);
    } else {
      itemRefs.current.delete(index);
    }
  }, []);

  return (
    <div ref={containerRef} className="h-full overflow-y-auto px-6 py-5">
      {messages.map((message, index) => {
        const isVisible = visibleSet.has(index);
        const cachedHeight = heightsRef.current.get(index);
        const placeholderHeight = cachedHeight ?? estimateHeight(message);

        return (
          <div
            key={message.id}
            ref={(el) => setItemRef(el, index)}
            data-index={index}
            data-message-index={index}
            className="overflow-hidden"
            style={{ minHeight: placeholderHeight }}
          >
            {isVisible ? (
              <MessageBubble message={message} messageIndex={index} />
            ) : (
              <div style={{ height: placeholderHeight }} />
            )}
          </div>
        );
      })}
    </div>
  );
});
