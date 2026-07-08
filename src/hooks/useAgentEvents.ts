import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useChatStore } from "@/stores/chatStore";
import { useAgentStore } from "@/stores/agentStore";
import { useSessionStore } from "@/stores/sessionStore";
import { initMcpStore, useMcpStore } from "@/stores/mcpStore";
import type { AskUserAnswer } from "@/types/tool";
import {
  AgentApprovalRequestPayload,
  AgentAskUserPromptPayload,
  AgentErrorPayload,
  AgentStatusPayload,
  AgentStreamDeltaPayload,
  AgentStreamDonePayload,
  AgentTokenPayload,
  AgentToolRecordPayload,
  AgentToolRejectedPayload,
  AgentDonePayload,
  EVT_ASK_USER_PROMPT,
  EVT_APPROVAL_REQUEST,
  EVT_DONE,
  EVT_ERROR,
  EVT_PARTIAL_ASSISTANT,
  EVT_STATUS,
  EVT_STREAM_DELTA,
  EVT_STREAM_DONE,
  EVT_TOKEN,
  EVT_TOOL_REJECTED,
  EVT_TOOL_RECORD,
  EVT_SESSION_CREATED,
  EVT_SESSION_UPDATED,
  EVT_SESSION_DELETED,
  EVT_SESSION_STATE,
  type SessionCreatedPayload,
  type SessionUpdatedPayload,
  type SessionDeletedPayload,
  type SessionStatePayload,
} from "@/types/event";
import { EVT_MCP_SERVER_STATE, type McpServerStatePayload } from "@/types/mcp";

// 监听 Rust Agent 推送到前端的所有事件，并按 conversationId 路由到对应 session 的 store。
//
// Phase 3.2 变化：
// - 所有 agent 事件 payload 加 conversationId，按 conv 路由到 chatStore/agentStore/sessionStore
// - 新增 4 个 session 事件（created/updated/deleted/state）
// - approval_request 不是 active session 时不弹 modal，只在 SessionItem 上显示红点 badge

export function useAgentEvents() {
  useEffect(() => {
    let cancelled = false;
    const unlisteners: UnlistenFn[] = [];

    void (async () => {
      try {
        const handlers: Array<
          [string, (e: { payload: unknown }) => void]
        > = [
          // ---- Agent events (按 conversationId 路由到对应 session) ----

          [
            EVT_TOKEN,
            (e) => {
              const p = e.payload as AgentTokenPayload;
              const chat = useChatStore.getState();
              chat.prepareAssistantMessageFor(p.conversationId, p.msgId);
              chat.appendTokenTo(p.conversationId, p.msgId, p.text);
            },
          ],
          [
            EVT_STATUS,
            (e) => {
              const p = e.payload as AgentStatusPayload;
              useAgentStore.getState().setStateFor(p.conversationId, p.state);
              // 更新 sessionStore 生成中标记
              const sessionStore = useSessionStore.getState();
              if (p.state === "Idle") {
                sessionStore.markGenerating(p.conversationId, false);
              } else {
                sessionStore.markGenerating(p.conversationId, true);
              }
            },
          ],
          [
            EVT_ERROR,
            (e) => {
              const p = e.payload as AgentErrorPayload;
              useAgentStore.getState().setErrorFor(p.conversationId, p.message);
              useChatStore.getState().markErrorFor(p.conversationId, p.msgId, p.message);
              useSessionStore.getState().markGenerating(p.conversationId, false);
            },
          ],
          [
            EVT_DONE,
            (e) => {
              const p = e.payload as AgentDonePayload;
              useChatStore.getState().markCompleteFor(p.conversationId, p.msgId);
            },
          ],

          // Phase 2 new
          [
            EVT_STREAM_DELTA,
            (e) => {
              const p = e.payload as AgentStreamDeltaPayload;
              const chat = useChatStore.getState();
              chat.prepareAssistantMessageFor(p.conversationId, p.msgId);
              chat.appendStreamDeltaTo(p.conversationId, p.msgId, p.text);
            },
          ],
          [
            EVT_STREAM_DONE,
            (e) => {
              const p = e.payload as AgentStreamDonePayload;
              useChatStore.getState().markCompleteFor(p.conversationId, p.msgId);
            },
          ],
          [
            EVT_TOOL_RECORD,
            (e) => {
              const p = e.payload as AgentToolRecordPayload;
              useChatStore.getState().upsertToolRecordTo(
                p.conversationId,
                p.runId,
                p.record,
              );
            },
          ],
          [
            EVT_APPROVAL_REQUEST,
            (e) => {
              const p = e.payload as AgentApprovalRequestPayload;
              const agentStore = useAgentStore.getState();
              const sessionStore = useSessionStore.getState();
              const activeId = sessionStore.activeSessionId;
               
              console.log("[approval_request] conv=", p.conversationId, "active=", activeId, "agentActive=", agentStore.activeConversationId, "tool=", p.toolName);
              // 注册 pending（分桶）
              agentStore.setApprovalRequestFor(p.conversationId, p);
              sessionStore.addPendingApproval(p.conversationId);
              // active session：确保顶层 approvalRequest 同步以触发 modal
              if (p.conversationId === activeId) {
                agentStore.setApprovalRequest(p);
              }
            },
          ],
          [
            EVT_ASK_USER_PROMPT,
            (e) => {
              const p = e.payload as AgentAskUserPromptPayload;
              useAgentStore.getState().setAskUserPromptFor(p.conversationId, p);
            },
          ],
          [
            EVT_TOOL_REJECTED,
            (e) => {
              const p = e.payload as AgentToolRejectedPayload;
              const now = Date.now();
              useChatStore.getState().upsertToolRecordTo(p.conversationId, p.runId, {
                id: p.toolCallId,
                name: p.toolName,
                source: "native",
                arguments: "",
                status: "Cancelled",
                error: `rejected: ${p.reason}`,
                startedAt: now,
                completedAt: now,
                round: 0,
                sensitive: false,
                artifacts: [],
              });
            },
          ],
          [
            EVT_PARTIAL_ASSISTANT,
            (_e) => {
              // Phase 2 stub：JSONL 持久化由后端处理
            },
          ],

          // ---- Phase 3.2: Session events ----

          [
            EVT_SESSION_CREATED,
            (e) => {
              const p = e.payload as SessionCreatedPayload;
              const conv = p.conversation;
              useSessionStore.setState((state) => {
                const item = {
                  id: conv.id,
                  title: conv.title,
                  preview: "",
                  createdAt: conv.createdAt,
                  updatedAt: conv.updatedAt,
                  pinned: conv.pinned,
                  messageCount: conv.messageCount,
                };
                if (state.sessions.find((s) => s.id === conv.id)) return {};
                const next = [...state.sessions, item];
                next.sort((a, b) => {
                  if (a.pinned !== b.pinned)
                    return (b.pinned ? 1 : 0) - (a.pinned ? 1 : 0);
                  return b.updatedAt - a.updatedAt;
                });
                return { sessions: next };
              });
            },
          ],
          [
            EVT_SESSION_UPDATED,
            (e) => {
              const p = e.payload as SessionUpdatedPayload;
              const conv = p.conversation;
              useSessionStore.setState((state) => {
                const next = state.sessions.map((s) =>
                  s.id === conv.id
                    ? {
                        ...s,
                        title: conv.title,
                        updatedAt: conv.updatedAt,
                        pinned: conv.pinned,
                        messageCount: conv.messageCount,
                      }
                    : s,
                );
                next.sort((a, b) => {
                  if (a.pinned !== b.pinned)
                    return (b.pinned ? 1 : 0) - (a.pinned ? 1 : 0);
                  return b.updatedAt - a.updatedAt;
                });
                return { sessions: next };
              });
            },
          ],
          [
            EVT_SESSION_DELETED,
            (e) => {
              const p = e.payload as SessionDeletedPayload;
              useSessionStore.setState((state) => {
                const next = state.sessions.filter((s) => s.id !== p.conversationId);
                const nextActive =
                  state.activeSessionId === p.conversationId
                    ? next.length > 0
                      ? next[0].id
                      : null
                    : state.activeSessionId;
                return { sessions: next, activeSessionId: nextActive };
              });
            },
          ],
          [
            EVT_SESSION_STATE,
            (e) => {
              const p = e.payload as SessionStatePayload;
              useAgentStore.getState().setStateFor(p.conversationId, p.state);
              useSessionStore.getState().markGenerating(
                p.conversationId,
                p.state !== "Idle",
              );
            },
          ],

          // Phase 3.1: MCP server 状态事件
          [
            EVT_MCP_SERVER_STATE,
            (e) => {
              const p = e.payload as McpServerStatePayload;
              useMcpStore.getState().setState(p.serverId, p.state);
            },
          ],
        ];

        // Phase 3.1: 启动时拉取一次 MCP server 列表 + 状态快照
        void initMcpStore();

        // 逐个订阅，注册一个 push 一个，这样 cleanup 时即使 async 还没全跑完
        // 也能取消已注册的；用 cancelled flag 防止 StrictMode 双订阅。
        for (const [name, handler] of handlers) {
          if (cancelled) return;
          const unlisten = await listen<unknown>(name, handler);
          if (cancelled) {
            unlisten();
            return;
          }
          unlisteners.push(unlisten);
        }
      } catch (err) {
        // 非 Tauri 环境（vite dev 单独跑）下，invoke/listen 不可用，静默忽略
         
        console.warn("[useAgentEvents] Tauri APIs unavailable:", err);
      }
    })();

    return () => {
      cancelled = true;
      unlisteners.forEach((u) => u());
    };
  }, []);
}

// ---- Command helpers ----

export interface SendMessageArgs {
  conversationId: string;
  text: string;
  runId: string;
  messageId: string;
}

/**
 * 发送一条用户消息给后端 run_agent_loop。
 * Phase 3.2: 前端生成 messageId（pending assistant 消息的 id）+ runId，
 * 后端用 messageId 作为 emit 事件的 msg_id，确保前端能正确路由 stream_delta。
 */
export async function sendMessage({
  conversationId,
  text,
  runId,
  messageId,
}: SendMessageArgs): Promise<{ success: boolean; error?: string }> {
  return await invoke<{ success: boolean; error?: string }>("send_message", {
    conversationId,
    text,
    runId,
    messageId,
  });
}

export async function approveTool(
  conversationId: string,
  approvalId: string,
  allow: boolean,
) {
  // 后端命令签名为 args: ApproveToolArgs，需要包在 args key 下
  await invoke("approve_tool", {
    args: { conversationId, approvalId, allow },
  });
  // 本地清掉 pending
  const agentStore = useAgentStore.getState();
  const sessionStore = useSessionStore.getState();
  agentStore.setApprovalRequestFor(conversationId, null);
  sessionStore.removePendingApproval(conversationId);
}

export async function answerAskUser(
  conversationId: string,
  askUserId: string,
  phase: string,
  answers: Record<string, AskUserAnswer>,
) {
  // 后端命令签名为 args: AnswerAskUserArgs，需要包在 args key 下
  await invoke("answer_ask_user", {
    args: {
      conversationId,
      askUserId,
      response: { phase, answers },
    },
  });
}

export async function cancelRun(conversationId: string) {
  await invoke("cancel_run", { conversationId });
}
