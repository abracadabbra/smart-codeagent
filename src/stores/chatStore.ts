import { create } from "zustand";
import type { Message } from "@/types/message";

interface ChatState {
  messages: Message[];
  currentAssistantId: string | null;

  // 用户发送消息时调用：追加一条 user 消息并预留 assistant 占位
  appendUserMessage: (text: string) => { userId: string; assistantId: string };
  // assistant 创建好后（流开始前）注册一次
  prepareAssistantMessage: (id: string) => void;

  // 流式：累加 token 到当前 assistant message
  appendToken: (id: string, text: string) => void;

  // 流结束
  markComplete: (id: string) => void;

  // 出错
  markError: (id: string, error: string) => void;
}

const newId = (prefix: string) =>
  `${prefix}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;

export const useChatStore = create<ChatState>((set) => ({
  messages: [],
  currentAssistantId: null,

  appendUserMessage: (text) => {
    const userId = newId("user");
    const assistantId = newId("asst");

    set((state) => ({
      messages: [
        ...state.messages,
        {
          id: userId,
          role: "user",
          content: text,
          status: "complete",
          createdAt: Date.now(),
        },
        {
          id: assistantId,
          role: "assistant",
          content: "",
          status: "pending",
          createdAt: Date.now(),
        },
      ],
      currentAssistantId: assistantId,
    }));

    return { userId, assistantId };
  },

  prepareAssistantMessage: (id) => {
    set((state) => ({
      messages: state.messages.map((m) =>
        m.id === id ? { ...m, status: "streaming" } : m,
      ),
    }));
  },

  appendToken: (id, text) => {
    set((state) => ({
      messages: state.messages.map((m) =>
        m.id === id
          ? { ...m, content: m.content + text, status: "streaming" }
          : m,
      ),
    }));
  },

  markComplete: (id) => {
    set((state) => ({
      messages: state.messages.map((m) =>
        m.id === id ? { ...m, status: "complete" } : m,
      ),
      currentAssistantId: null,
    }));
  },

  markError: (id, error) => {
    set((state) => ({
      messages: state.messages.map((m) =>
        m.id === id
          ? { ...m, status: "error", error, content: m.content || error }
          : m,
      ),
      currentAssistantId: null,
    }));
  },
}));