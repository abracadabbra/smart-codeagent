//! Agent Loop 主循环：4 状态，Phase 1 简化版（无 Tool / Recover）。

use crate::agent::{AgentState, Message};
use crate::ipc::events::{emit_done, emit_error, emit_status, emit_token};
use crate::providers::{MessagesRequest, Provider};
use crate::providers::anthropic::AnthropicClient;
use futures::StreamExt;
use std::sync::Arc;
use tauri::AppHandle;
use tokio::sync::Mutex;
use tracing::{debug, info};

/// 全局共享的 Agent Loop handle。
/// Phase 1 单实例，不做 session 隔离。
pub struct AgentLoop {
    app: Mutex<Option<AppHandle>>,
    state: Mutex<AgentState>,
    history: Mutex<Vec<Message>>,
}

impl AgentLoop {
    pub fn new() -> Self {
        Self {
            app: Mutex::new(None),
            state: Mutex::new(AgentState::Idle),
            history: Mutex::new(Vec::new()),
        }
    }

    /// 命令入口第一次调用时注入 AppHandle，之后所有 emit 都用它。
    pub async fn attach_app(self: &Arc<Self>, handle: AppHandle) {
        let mut slot = self.app.lock().await;
        *slot = Some(handle);
    }

    pub async fn app_handle(&self) -> Option<AppHandle> {
        self.app.lock().await.clone()
    }

    pub async fn current_state(&self) -> AgentState {
        *self.state.lock().await
    }

    /// 收到 send_message command 时调用：跑完一轮 Idle→Prepare→Stream→Stop→Idle。
    /// Phase 1 不阻塞 command 调用 — 用 tokio::spawn 在后台执行，command 立即返回。
    pub fn spawn_run(self: Arc<Self>, text: String, assistant_id: String) {
        tokio::spawn(async move {
            if let Err(e) = self.run_inner(text, assistant_id).await {
                tracing::error!("agent loop failed: {e:?}");
            }
        });
    }

    async fn run_inner(self: Arc<Self>, user_text: String, assistant_id: String) -> anyhow::Result<()> {
        // 1. 状态转移：Idle → Prepare
        self.transition(AgentState::Prepare).await;
        emit_status(self.app_handle().await.as_ref(), AgentState::Prepare);

        // 2. 构造消息历史（追加用户消息）
        let messages = {
            let mut history = self.history.lock().await;
            history.push(Message {
                role: "user".into(),
                content: user_text,
            });
            history.clone()
        };

        // 3. 构造 provider 并发起请求
        let config = crate::config::AnthropicConfig::from_env();
        let client = AnthropicClient::new(config);
        let req = MessagesRequest {
            model: client.config().model.clone(),
            max_tokens: 8192,
            messages: messages.clone(),
            system: Some(
                "You are Smart CodeAgent, an AI coding assistant. Be concise and helpful."
                    .to_string(),
            ),
            stream: true,
        };

        // 4. 状态转移：Prepare → Stream
        self.transition(AgentState::Stream).await;
        emit_status(self.app_handle().await.as_ref(), AgentState::Stream);

        // 5. 流式消费
        let stream_result = client.stream_chat(req).await;
        match stream_result {
            Ok(mut s) => {
                let mut full_text = String::new();
                while let Some(item) = s.next().await {
                    match item {
                        Ok(delta) => {
                            // 推送 token 给前端
                            emit_token(
                                self.app_handle().await.as_ref(),
                                &assistant_id,
                                &delta,
                            );
                            full_text.push_str(&delta);
                        }
                        Err(e) => {
                            emit_error(
                                self.app_handle().await.as_ref(),
                                &assistant_id,
                                &format!("stream error: {e}"),
                            );
                            self.transition(AgentState::Stop).await;
                            emit_status(self.app_handle().await.as_ref(), AgentState::Stop);
                            return Err(anyhow::anyhow!(e));
                        }
                    }
                }
                debug!("collected full response: {full_text}");

                // 把完整回复追加进历史
                self.history.lock().await.push(Message {
                    role: "assistant".into(),
                    content: full_text,
                });
            }
            Err(e) => {
                // 启动流就失败（401、网络断开等）
                emit_error(
                    self.app_handle().await.as_ref(),
                    &assistant_id,
                    &format!("request failed: {e}"),
                );
                self.transition(AgentState::Stop).await;
                emit_status(self.app_handle().await.as_ref(), AgentState::Stop);
                return Err(anyhow::anyhow!(e));
            }
        }

        // 6. 状态转移：Stream → Stop → Idle
        self.transition(AgentState::Stop).await;
        emit_status(self.app_handle().await.as_ref(), AgentState::Stop);
        emit_done(self.app_handle().await.as_ref(), &assistant_id);

        self.transition(AgentState::Idle).await;
        emit_status(self.app_handle().await.as_ref(), AgentState::Idle);

        info!("agent loop run completed");
        Ok(())
    }

    async fn transition(&self, new_state: AgentState) {
        let mut s = self.state.lock().await;
        *s = new_state;
    }
}