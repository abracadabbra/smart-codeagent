//! OpenAI Chat Completions 兼容 Provider（流式 + tool calling）。
//!
//! 协议：POST {base_url}/v1/chat/completions，body.stream=true，
//! Accept: text/event-stream。
//!
//! 为什么不用 Anthropic `/v1/messages`：
//! - SenseNova 的 `/v1/messages` 只对 Claude 系列模型翻译 tool_use content block；
//! - 对 DeepSeek / Qwen 等模型，tool_use 被退化成纯文本输出（`call_` ID 而非 `toolu_`）；
//! - `/v1/chat/completions` 是 OpenAI 标准，DeepSeek / Qwen / Claude（via proxy）
//!   都原生支持 `delta.tool_calls` 流式 tool calling。
//!
//! SSE 事件序列（OpenAI streaming）：
//! ```text
//! data: {"choices":[{"delta":{"role":"assistant"}, "index":0}]}
//! data: {"choices":[{"delta":{"content":"hi"}, "index":0}]}
//! data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_xxx","type":"function","function":{"name":"read_file","arguments":""}}]}, "index":0}]}
//! data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"path\":"}}]}, "index":0}]}
//! data: {"choices":[{"delta":{},"finish_reason":"tool_calls","index":0}]}
//! data: [DONE]
//! ```
//!
//! tool_calls 流式特点：
//! - 第一块带 `id` + `function.name` + 空 `arguments`
//! - 后续块只有 `function.arguments` 增量（JSON 字符串片段）
//! - `finish_reason: "tool_calls"` 标志所有 tool_call 结束
//! - `index` 字段区分并行 tool_calls（0, 1, 2...）

use crate::config::AnthropicConfig;
use crate::providers::{MessagesRequest, Provider, ProviderError, ProviderResult, StreamChunk, TokenStream};
use async_stream::try_stream;
use futures::stream::StreamExt;
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;

/// OpenAI 兼容客户端（文件名保留 `anthropic.rs` 是历史原因 + 避免大面积改 mod 引用）。
pub struct AnthropicClient {
    cfg: AnthropicConfig,
    http: Client,
}

impl AnthropicClient {
    pub fn new(cfg: AnthropicConfig) -> Self {
        let http = Client::builder()
            .build()
            .expect("reqwest client should build");
        Self { cfg, http }
    }

    pub fn config(&self) -> &AnthropicConfig {
        &self.cfg
    }
}

/// OpenAI SSE data 行的 JSON 结构（只取关心的字段）。
#[derive(Debug, Deserialize)]
struct SseData {
    #[serde(default)]
    choices: Vec<SseChoice>,
    #[serde(default)]
    error: Option<SseError>,
}

#[derive(Debug, Deserialize)]
struct SseChoice {
    #[serde(default)]
    delta: Option<SseDelta>,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SseDelta {
    #[serde(default)]
    content: Option<String>,
    /// OpenAI tool_calls 流式块（按 index 聚合）
    #[serde(default)]
    tool_calls: Option<Vec<SseToolCallDelta>>,
}

/// 单个 tool_call 的流式增量。
/// 第一块带 id + function.name，后续块只带 function.arguments。
#[derive(Debug, Deserialize)]
struct SseToolCallDelta {
    /// tool_call 在本轮响应中的序号（0-based），用于区分并行 tool_calls
    #[serde(default)]
    index: u32,
    #[serde(default)]
    id: Option<String>,
    #[serde(default, rename = "type")]
    call_type: Option<String>,
    #[serde(default)]
    function: Option<SseFunctionDelta>,
}

#[derive(Debug, Deserialize)]
struct SseFunctionDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SseError {
    #[serde(default)]
    message: String,
}

#[async_trait::async_trait]
impl Provider for AnthropicClient {
    async fn stream_chat(&self, req: MessagesRequest) -> ProviderResult<TokenStream> {
        let url = format!("{}/v1/chat/completions", self.cfg.base_url);

        // tools: 转成 OpenAI function calling 格式
        // [{"type":"function","function":{"name","description","parameters":input_schema}}]
        let tools_api: Vec<serde_json::Value> = if req.tools.is_empty() {
            Vec::new()
        } else {
            req.tools
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "type": "function",
                        "function": {
                            "name": t.name,
                            "description": t.description,
                            "parameters": t.input_schema,
                        }
                    })
                })
                .collect()
        };

        // messages: 系统提示作为第一条 system 消息（OpenAI 不用顶层 system 字段）
        let mut messages_api: Vec<serde_json::Value> = Vec::new();
        if let Some(system) = req.system {
            messages_api.push(serde_json::json!({
                "role": "system",
                "content": system,
            }));
        }
        for m in &req.messages {
            // Message 已经是 OpenAI 格式（serde 序列化），但 content 是 Option<String>
            // OpenAI 要求 content 至少是 null 或字符串，serde 会把 None 序列化成 null
            let mut msg = serde_json::to_value(m).unwrap_or_else(|_| {
                serde_json::json!({"role": m.role, "content": ""})
            });
            // 如果 content 是 None 且有 tool_calls，确保 content 字段存在为 null
            if m.content.is_none() {
                msg["content"] = serde_json::Value::Null;
            }
            messages_api.push(msg);
        }

        let tools_count = tools_api.len();
        let mut body = serde_json::json!({
            "model": req.model,
            "max_tokens": req.max_tokens,
            "messages": messages_api,
            "stream": req.stream,
        });
        if !tools_api.is_empty() {
            body["tools"] = serde_json::Value::Array(tools_api);
        }

        tracing::info!(
            "POST {} | model={} | messages={} | tools={}",
            url, req.model, req.messages.len(), tools_count
        );
        tracing::debug!("request body: {}", body);

        let response = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.cfg.api_key))
            .header("content-type", "application/json")
            .header("accept", "text/event-stream")
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Http(e.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            tracing::warn!(
                "API error: status={} | body_len={} | body_preview={}",
                status,
                body.len(),
                if body.len() > 300 { &body[..300] } else { &body }
            );
            return Err(ProviderError::Api {
                status: status.as_u16(),
                message: body,
            });
        }
        tracing::info!("LLM API 200 OK, starting SSE stream");

        let mut byte_stream = response.bytes_stream();
        let mut buf = String::new();

        let stream = try_stream! {
            // tool_call 聚合状态：index → (id, name, arguments_buf)
            // 用于在 finish_reason="tool_calls" 时一次性 emit 完整的 ToolUseStart + InputDelta + End
            //
            // 但为了流式体验更好，我们在收到第一块（带 id+name）时立即 emit ToolUseStart，
            // 后续 arguments 增量直接 emit ToolUseInputDelta。
            // 这里只记 index → has_started 标志，避免重复 emit Start。
            let mut tool_started: HashMap<u32, bool> = HashMap::new();

            while let Some(chunk_res) = byte_stream.next().await {
                let chunk = chunk_res.map_err(|e| ProviderError::Http(e.to_string()))?;
                buf.push_str(&String::from_utf8_lossy(&chunk));

                while let Some(idx) = buf.find("\n\n") {
                    let event_text: String = buf.drain(..idx + 2).collect();
                    for c in parse_event(&event_text, &mut tool_started) {
                        yield c;
                    }
                }
            }
            // 收尾 flush
            if !buf.is_empty() {
                for c in parse_event(&buf, &mut tool_started) {
                    yield c;
                }
            }
        };

        Ok(Box::pin(stream))
    }
}

/// 解析一个 OpenAI SSE event 文本块，产出 0..N 个 StreamChunk。
///
/// `tool_started` 跟踪每个 tool_call index 是否已 emit ToolUseStart，
/// 避免同一个 tool_call 被重复 Start。
fn parse_event(event_text: &str, tool_started: &mut HashMap<u32, bool>) -> Vec<StreamChunk> {
    // 一个 SSE event 可能有多行 data:（OpenAI 每条 data 是一个完整 JSON）
    let mut out = Vec::new();

    for line in event_text.lines() {
        let line = line.trim_end_matches('\r');
        let Some(rest) = line.strip_prefix("data:") else {
            continue;
        };
        let raw = rest.trim();
        if raw == "[DONE]" {
            out.push(StreamChunk::Done { stop_reason: None });
            continue;
        }

        let data: SseData = match serde_json::from_str(raw) {
            Ok(v) => v,
            Err(e) => {
                tracing::debug!("skip non-JSON SSE payload: {e}");
                continue;
            }
        };

        if let Some(err) = data.error {
            tracing::warn!("API error in SSE: {}", err.message);
            out.push(StreamChunk::Done {
                stop_reason: Some(format!("api_error: {}", err.message)),
            });
            continue;
        }

        for choice in data.choices {
            // 处理 finish_reason —— SenseNova 流式响应里很多块带空字符串 `""`，
            // 只有真正的结束信号才是非空字符串（"stop"/"tool_calls"/"length"/"content_filter"）。
            // 空字符串当成 None 处理，避免提前触发 Done。
            let reason_opt = choice
                .finish_reason
                .as_deref()
                .filter(|s| !s.is_empty());

            if let Some(reason) = reason_opt {
                match reason {
                    "tool_calls" => {
                        // 所有 tool_call 结束 —— emit 一个 ToolUseEnd 表示
                        // "本轮 tool_calls 全部结束"（OpenAI 是全部一起结束）
                        out.push(StreamChunk::ToolUseEnd);
                        out.push(StreamChunk::Done {
                            stop_reason: Some("tool_calls".into()),
                        });
                    }
                    "stop" | "length" | "content_filter" => {
                        out.push(StreamChunk::Done {
                            stop_reason: Some(reason.into()),
                        });
                    }
                    _ => {
                        // 其他 finish_reason 透传
                        out.push(StreamChunk::Done {
                            stop_reason: Some(reason.into()),
                        });
                    }
                }
            }

            let Some(delta) = choice.delta else {
                continue;
            };

            // 1. 文本增量（content 字段；reasoning_content 字段直接忽略 —— 不展示思考过程）
            if let Some(text) = delta.content.as_ref() {
                if !text.is_empty() {
                    out.push(StreamChunk::Text(text.clone()));
                }
            }

            // 2. tool_calls 增量
            if let Some(tool_calls) = delta.tool_calls {
                for tc in tool_calls {
                    let started = tool_started.get(&tc.index).copied().unwrap_or(false);

                    if !started {
                        // 第一块：带 id + function.name
                        if let (Some(id), Some(func)) = (tc.id.as_ref(), tc.function.as_ref()) {
                            if let Some(name) = func.name.as_ref() {
                                out.push(StreamChunk::ToolUseStart {
                                    id: id.clone(),
                                    name: name.clone(),
                                });
                                tool_started.insert(tc.index, true);
                                // 第一块可能也带 arguments 增量
                                if let Some(args) = func.arguments.as_ref() {
                                    if !args.is_empty() {
                                        out.push(StreamChunk::ToolUseInputDelta(args.clone()));
                                    }
                                }
                            }
                        }
                    } else {
                        // 后续块：只有 function.arguments 增量
                        if let Some(func) = tc.function.as_ref() {
                            if let Some(args) = func.arguments.as_ref() {
                                if !args.is_empty() {
                                    out.push(StreamChunk::ToolUseInputDelta(args.clone()));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{OpenAiFunction, OpenAiToolCall};

    #[test]
    fn parses_text_delta() {
        let event = "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hello\"}}]}\n\n";
        let mut started = HashMap::new();
        let chunks = parse_event(event, &mut started);
        assert_eq!(chunks.len(), 1);
        match &chunks[0] {
            StreamChunk::Text(t) => assert_eq!(t, "hello"),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn parses_tool_call_start_and_args() {
        // 第一块：id + name + 空 arguments
        let event1 = "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_01\",\"type\":\"function\",\"function\":{\"name\":\"read_file\",\"arguments\":\"\"}}]}}]}\n\n";
        // 第二块：arguments 增量
        let event2 = "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"path\\\":\\\"/tmp\\\"}\"}}]}}]}\n\n";
        let mut started = HashMap::new();

        let chunks1 = parse_event(event1, &mut started);
        assert_eq!(chunks1.len(), 1);
        match &chunks1[0] {
            StreamChunk::ToolUseStart { id, name } => {
                assert_eq!(id, "call_01");
                assert_eq!(name, "read_file");
            }
            other => panic!("expected ToolUseStart, got {other:?}"),
        }
        assert_eq!(started.get(&0), Some(&true));

        let chunks2 = parse_event(event2, &mut started);
        assert_eq!(chunks2.len(), 1);
        match &chunks2[0] {
            StreamChunk::ToolUseInputDelta(s) => assert_eq!(s, "{\"path\":\"/tmp\"}"),
            other => panic!("expected ToolUseInputDelta, got {other:?}"),
        }
    }

    #[test]
    fn parses_finish_reason_tool_calls() {
        let event = "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n";
        let mut started = HashMap::new();
        let chunks = parse_event(event, &mut started);
        assert_eq!(chunks.len(), 2);
        assert!(matches!(chunks[0], StreamChunk::ToolUseEnd));
        match &chunks[1] {
            StreamChunk::Done { stop_reason } => {
                assert_eq!(stop_reason.as_deref(), Some("tool_calls"));
            }
            other => panic!("expected Done, got {other:?}"),
        }
    }

    #[test]
    fn parses_finish_reason_stop() {
        let event = "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n";
        let mut started = HashMap::new();
        let chunks = parse_event(event, &mut started);
        assert_eq!(chunks.len(), 1);
        match &chunks[0] {
            StreamChunk::Done { stop_reason } => {
                assert_eq!(stop_reason.as_deref(), Some("stop"));
            }
            other => panic!("expected Done, got {other:?}"),
        }
    }

    #[test]
    fn parses_done_marker() {
        let mut started = HashMap::new();
        let chunks = parse_event("data: [DONE]\n\n", &mut started);
        assert_eq!(chunks.len(), 1);
        assert!(matches!(chunks[0], StreamChunk::Done { .. }));
    }

    #[test]
    fn handles_multiple_tool_calls_in_parallel() {
        // 两个并行 tool_calls，各自按 index 聚合
        let event1 = "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_a\",\"type\":\"function\",\"function\":{\"name\":\"read_file\",\"arguments\":\"\"}}]}}]}\n\n";
        let event2 = "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":1,\"id\":\"call_b\",\"type\":\"function\",\"function\":{\"name\":\"ls\",\"arguments\":\"\"}}]}}]}\n\n";
        let mut started = HashMap::new();

        let chunks1 = parse_event(event1, &mut started);
        assert_eq!(chunks1.len(), 1);
        match &chunks1[0] {
            StreamChunk::ToolUseStart { id, name } => {
                assert_eq!(id, "call_a");
                assert_eq!(name, "read_file");
            }
            other => panic!("got {other:?}"),
        }

        let chunks2 = parse_event(event2, &mut started);
        assert_eq!(chunks2.len(), 1);
        match &chunks2[0] {
            StreamChunk::ToolUseStart { id, name } => {
                assert_eq!(id, "call_b");
                assert_eq!(name, "ls");
            }
            other => panic!("got {other:?}"),
        }
    }

    /// 验证 OpenAiToolCall 序列化符合 OpenAI API 要求。
    #[test]
    fn openai_tool_call_serializes_correctly() {
        let tc = OpenAiToolCall {
            id: "call_abc".into(),
            call_type: "function".into(),
            function: OpenAiFunction {
                name: "read_file".into(),
                arguments: "{\"path\":\"/tmp\"}".into(),
            },
        };
        let v = serde_json::to_value(&tc).unwrap();
        assert_eq!(v["id"], "call_abc");
        assert_eq!(v["type"], "function");
        assert_eq!(v["function"]["name"], "read_file");
        assert_eq!(v["function"]["arguments"], "{\"path\":\"/tmp\"}");
    }

    /// 回归测试：Message 序列化后字段名必须是 snake_case
    /// （tool_calls / tool_call_id），不能是 camelCase。
    /// OpenAI API 严格校验字段名，camelCase 会让 tool_calls 被忽略，
    /// 紧跟的 tool message 就会因 "invalid tool_call_id" 被拒。
    #[test]
    fn message_serializes_openai_snake_case() {
        use crate::agent::Message;

        // assistant tool_calls message
        let msg = Message::assistant_tool_calls(vec![OpenAiToolCall {
            id: "call_x".into(),
            call_type: "function".into(),
            function: OpenAiFunction {
                name: "read_file".into(),
                arguments: "{}".into(),
            },
        }]);
        let v = serde_json::to_value(&msg).unwrap();
        assert_eq!(v["role"], "assistant");
        assert_eq!(v["content"], serde_json::Value::Null);
        assert!(
            v.get("tool_calls").is_some(),
            "tool_calls must be snake_case, got keys: {:?}",
            v.as_object().map(|o| o.keys().collect::<Vec<_>>())
        );
        assert!(
            v.get("toolCalls").is_none(),
            "camelCase toolCalls leaked into serialization"
        );

        // tool result message
        let msg = Message::tool_result("call_x", "result");
        let v = serde_json::to_value(&msg).unwrap();
        assert_eq!(v["role"], "tool");
        assert_eq!(v["content"], "result");
        assert!(
            v.get("tool_call_id").is_some(),
            "tool_call_id must be snake_case"
        );
        assert!(
            v.get("toolCallId").is_none(),
            "camelCase toolCallId leaked into serialization"
        );
    }
}
