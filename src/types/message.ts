export type Role = "user" | "assistant";

export type MessageStatus =
  | "pending"     // 用户已发送，等待后端确认
  | "streaming"   // 助手正在流式输出
  | "complete"    // 助手回复结束
  | "error";      // 出错

export interface Message {
  id: string;
  role: Role;
  content: string;
  status: MessageStatus;
  createdAt: number;
  error?: string;
}