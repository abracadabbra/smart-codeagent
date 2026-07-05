import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useChatStore } from "@/stores/chatStore";
import { useAgentStore } from "@/stores/agentStore";
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
} from "@/types/event";
import { EVT_MCP_SERVER_STATE, type McpServerStatePayload } from "@/types/mcp";

// 监听 Rust Agent 推送到前端的所有事件，并把事件映射到 Zustand stores。

export function useAgentEvents() {
  const appendToken = useChatStore((s) => s.appendToken);
  const appendStreamDelta = useChatStore((s) => s.appendStreamDelta);
  const prepareAssistantMessage = useChatStore(
    (s) => s.prepareAssistantMessage,
  );
  const markComplete = useChatStore((s) => s.markComplete);
  const markError = useChatStore((s) => s.markError);
  const upsertToolRecord = useChatStore((s) => s.upsertToolRecord);
  const setAgentState = useAgentStore((s) => s.setState);
  const setAgentError = useAgentStore((s) => s.setError);
  const setApprovalRequest = useAgentStore((s) => s.setApprovalRequest);
  const setAskUserPrompt = useAgentStore((s) => s.setAskUserPrompt);
  const clearPrompts = useAgentStore((s) => s.clearPrompts);

  useEffect(() => {
    let cancelled = false;
    const unlisteners: UnlistenFn[] = [];

    void (async () => {
      try {
        const handlers: Array<
          [string, (e: { payload: unknown }) => void]
        > = [
          // Phase 1 legacy
          [
            EVT_TOKEN,
            (e) => {
              const p = e.payload as AgentTokenPayload;
              prepareAssistantMessage(p.msgId);
              appendToken(p.msgId, p.text);
            },
          ],
          [
            EVT_STATUS,
            (e) => {
              const p = e.payload as AgentStatusPayload;
              setAgentState(p.state);
            },
          ],
          [
            EVT_ERROR,
            (e) => {
              const p = e.payload as AgentErrorPayload;
              setAgentError(p.message);
              markError(p.msgId, p.message);
            },
          ],
          [
            EVT_DONE,
            (e) => {
              const p = e.payload as AgentDonePayload;
              markComplete(p.msgId);
            },
          ],

          // Phase 2 new
          [
            EVT_STREAM_DELTA,
            (e) => {
              const p = e.payload as AgentStreamDeltaPayload;
              prepareAssistantMessage(p.msgId);
              appendStreamDelta(p.msgId, p.text);
            },
          ],
          [
            EVT_STREAM_DONE,
            (e) => {
              const p = e.payload as AgentStreamDonePayload;
              markComplete(p.msgId);
            },
          ],
          [
            EVT_TOOL_RECORD,
            (e) => {
              const p = e.payload as AgentToolRecordPayload;
              upsertToolRecord(p.runId, p.record);
            },
          ],
          [
            EVT_APPROVAL_REQUEST,
            (e) => {
              const p = e.payload as AgentApprovalRequestPayload;
              setApprovalRequest(p);
            },
          ],
          [
            EVT_ASK_USER_PROMPT,
            (e) => {
              const p = e.payload as AgentAskUserPromptPayload;
              setAskUserPrompt(p);
            },
          ],
          [
            EVT_TOOL_REJECTED,
            (e) => {
              const p = e.payload as AgentToolRejectedPayload;
              // rejection 在 store 里以 ToolCallRecord 形式落地（Cancelled 状态 + reason）
              // — 直接通过 appendToolRecord 一致化处理
              const now = Date.now();
              upsertToolRecord(p.runId, {
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
              // Phase 2 stub：内存态 agent loop 不需要 persist
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
        // eslint-disable-next-line no-console
        console.warn("[useAgentEvents] Tauri APIs unavailable:", err);
      }
    })();

    return () => {
      cancelled = true;
      unlisteners.forEach((u) => u());
      clearPrompts();
    };
  }, [
    appendToken,
    appendStreamDelta,
    prepareAssistantMessage,
    markComplete,
    markError,
    upsertToolRecord,
    setAgentState,
    setAgentError,
    setApprovalRequest,
    setAskUserPrompt,
    clearPrompts,
  ]);
}

export interface SendMessageArgs {
  text: string;
  assistantId: string;
  runId: string;
  generation: number;
}

/**
 * 发送一条用户消息给后端 Agent Loop。
 * `runId` 前端生成（避免后端重复）；`generation` 是 runId 的递增版本号。
 */
export async function sendMessage({
  text,
  assistantId,
  runId,
  generation,
}: SendMessageArgs) {
  await invoke("send_message", {
    text,
    assistantId,
    runId,
    generation,
  });
}

export async function approveTool(approvalId: string, allow: boolean) {
  await invoke("approve_tool", {
    args: { approvalId, allow },
  });
}

export async function answerAskUser(
  askUserId: string,
  phase: string,
  answers: Record<string, AskUserAnswer>,
) {
  await invoke("answer_ask_user", {
    args: { askUserId, response: { phase, answers } },
  });
}

export async function cancelRun(runId: string) {
  await invoke("cancel_run", { runId });
}