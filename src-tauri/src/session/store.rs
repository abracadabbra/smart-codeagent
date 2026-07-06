//! `SessionStore` —— 会话持久化的内存缓存 + 写穿磁盘层。
//!
//! design.md §4 的实现。职责：
//! - 内存缓存：`RwLock<HashMap<conv_id, ConversationData>>`，活跃会话全在内存
//! - 写穿一致性：`append_message` 先 append 磁盘成功再更新内存；`update_meta` 先 atomic_write 再更新内存
//! - 懒加载：`load_messages_paged` 支持游标分页（默认 50 条）
//! - 崩溃恢复：`messages.jsonl` 逐行解析，损坏行跳过 + warn
//!
//! 并发模型（design.md §4.2）：
//! - `RwLock` 允许多 session 并行读；写时独占
//! - 同一 session 同一时间只有一个 run（由 `AppState.try_reserve_chat_send` 守门）
//! - 跨 session 多 run 并行（各自操作不同 conv_id）

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

use tauri::{AppHandle, Manager};
use tokio::sync::RwLock;

use crate::session::storage::{
    atomic_write, validate_conversation_id,
};
use crate::session::types::{ChatMessage, Conversation, ConversationListItem, SessionMessagesPage};

/// 默认懒加载分页大小。
const DEFAULT_PAGE_LIMIT: usize = 50;

/// 单个会话的内存数据。
pub struct ConversationData {
    /// 会话元数据（meta.json 的内存镜像）
    pub meta: Conversation,
    /// 消息列表（messages.jsonl 的内存镜像；懒加载，首次访问时从磁盘读）
    pub messages: Vec<ChatMessage>,
    /// 是否已从磁盘加载 messages（false 表示 messages 为空但未加载）
    pub loaded: bool,
}

/// 会话存储：内存缓存 + 写穿磁盘。
///
/// 全局单例，通过 `app.manage(Arc::new(SessionStore::new(...)))` 注册。
/// agent loop 通过 `Arc<SessionStore>` 共享访问。
pub struct SessionStore {
    /// 内存缓存：conv_id → ConversationData
    sessions: RwLock<HashMap<String, ConversationData>>,
    /// sessions/ 目录路径
    base_dir: PathBuf,
}

impl SessionStore {
    /// 构造空 store（不加载任何数据，需显式调 `load_index`）。
    pub fn new(base_dir: PathBuf) -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            base_dir,
        }
    }

    /// 从 AppHandle 构造。
    ///
    /// 路径优先级：
    /// 1. 环境变量 `SMARTCODEAGENT_SESSIONS_DIR`（dev 覆盖用，绕过 macOS App Sandbox / TRAE 沙箱）
    /// 2. `<app_data_dir>/sessions/`（默认，生产路径）
    pub fn from_app(app: &AppHandle) -> Result<Self, String> {
        if let Ok(custom) = std::env::var("SMARTCODEAGENT_SESSIONS_DIR") {
            if !custom.is_empty() {
                let base = std::path::PathBuf::from(custom);
                return Ok(Self::new(base));
            }
        }
        let base = app
            .path()
            .app_data_dir()
            .map_err(|e| format!("app_data_dir 不可用: {e}"))?
            .join("sessions");
        Ok(Self::new(base))
    }

    /// base_dir getter（测试用）。
    pub fn base_dir(&self) -> &PathBuf {
        &self.base_dir
    }

    // -----------------------------------------------------------------------
    // 列表加载
    // -----------------------------------------------------------------------

    /// 启动时加载会话列表。
    ///
    /// 策略（design.md §2.4）：
    /// 1. 优先读 `index.json`（一次 IO 拿全部摘要）
    /// 2. 若 index.json 不存在或损坏 → 扫 `sessions/*/meta.json` 重建
    /// 3. 把所有 meta 加入内存缓存（messages 不加载，懒加载）
    ///
    /// 返回按 `pinned desc, updatedAt desc` 排序的列表。
    pub async fn load_index(&self) -> Result<Vec<ConversationListItem>, String> {
        // 确保 base_dir 存在
        if !self.base_dir.exists() {
            fs::create_dir_all(&self.base_dir)
                .map_err(|e| format!("create sessions dir: {e}"))?;
        }

        let items = self.read_index_file().await.unwrap_or_else(|err| {
            tracing::warn!("load_index: index.json 不可用 ({err}), 扫描 sessions/*/meta.json");
            self.scan_meta_files().unwrap_or_default()
        });

        // 填充内存缓存（只 meta，messages 懒加载）
        let mut sessions = self.sessions.write().await;
        sessions.clear();
        for item in &items {
            sessions.insert(
                item.id.clone(),
                ConversationData {
                    meta: Conversation {
                        id: item.id.clone(),
                        title: item.title.clone(),
                        created_at: item.created_at,
                        updated_at: item.updated_at,
                        pinned: item.pinned,
                        message_count: item.message_count,
                    },
                    messages: vec![],
                    loaded: false,
                },
            );
        }
        drop(sessions);

        // 排序：pinned desc, updatedAt desc
        let mut sorted = items;
        sorted.sort_by(|a, b| {
            if a.pinned != b.pinned {
                return b.pinned.cmp(&a.pinned);
            }
            b.updated_at.cmp(&a.updated_at)
        });
        Ok(sorted)
    }

    /// 读 `index.json`。
    async fn read_index_file(&self) -> Result<Vec<ConversationListItem>, String> {
        let path = self.base_dir.join("index.json");
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("read index.json: {e}"))?;
        let items: Vec<ConversationListItem> = serde_json::from_str(&content)
            .map_err(|e| format!("parse index.json: {e}"))?;
        Ok(items)
    }

    /// 扫描 `sessions/*/meta.json` 重建列表（index.json 损坏时的 fallback）。
    fn scan_meta_files(&self) -> Result<Vec<ConversationListItem>, String> {
        let mut items = vec![];
        if !self.base_dir.exists() {
            return Ok(items);
        }
        for entry in fs::read_dir(&self.base_dir)
            .map_err(|e| format!("read sessions dir: {e}"))?
        {
            let entry = entry.map_err(|e| format!("read dir entry: {e}"))?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let meta_path = path.join("meta.json");
            if !meta_path.exists() {
                continue;
            }
            let content = match fs::read_to_string(&meta_path) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("scan_meta_files: skip {}: {e}", meta_path.display());
                    continue;
                }
            };
            let meta: Conversation = match serde_json::from_str(&content) {
                Ok(m) => m,
                Err(e) => {
                    tracing::warn!("scan_meta_files: skip {}: parse error: {e}", meta_path.display());
                    continue;
                }
            };
            items.push(ConversationListItem {
                id: meta.id,
                title: meta.title,
                preview: String::new(), // scan 模式不读 messages，preview 留空
                created_at: meta.created_at,
                updated_at: meta.updated_at,
                pinned: meta.pinned,
                message_count: meta.message_count,
            });
        }
        Ok(items)
    }

    // -----------------------------------------------------------------------
    // CRUD
    // -----------------------------------------------------------------------

    /// 创建新会话。
    ///
    /// 生成 `conv_{uuid_v4}`，写 `meta.json` + 更新 `index.json` + 加入内存缓存。
    pub async fn create_session(&self) -> Result<Conversation, String> {
        let conv_id = format!("conv_{}", uuid::Uuid::new_v4());
        validate_conversation_id(&conv_id)?;
        let now = chrono::Utc::now().timestamp_millis();
        let conv = Conversation {
            id: conv_id.clone(),
            title: "New Session".into(),
            created_at: now,
            updated_at: now,
            pinned: false,
            message_count: 0,
        };

        // 写 meta.json
        let meta_path = self.meta_path(&conv_id)?;
        let meta_json = serde_json::to_string_pretty(&conv)
            .map_err(|e| format!("serialize meta: {e}"))?;
        atomic_write(&meta_path, &meta_json, "meta")?;

        // 更新内存缓存
        {
            let mut sessions = self.sessions.write().await;
            sessions.insert(
                conv_id.clone(),
                ConversationData {
                    meta: conv.clone(),
                    messages: vec![],
                    loaded: true, // 新会话无消息，标记为已加载
                },
            );
        }

        // 更新 index.json
        self.rewrite_index().await?;

        Ok(conv)
    }

    /// 获取会话 meta（从缓存）。
    pub async fn get_meta(&self, conv_id: &str) -> Result<Conversation, String> {
        validate_conversation_id(conv_id)?;
        let sessions = self.sessions.read().await;
        sessions
            .get(conv_id)
            .map(|d| d.meta.clone())
            .ok_or_else(|| format!("session not found: {conv_id}"))
    }

    /// 加载会话所有消息（懒加载：首次访问时从 messages.jsonl 读）。
    pub async fn load_messages(&self, conv_id: &str) -> Result<Vec<ChatMessage>, String> {
        validate_conversation_id(conv_id)?;
        // 检查缓存
        {
            let sessions = self.sessions.read().await;
            if let Some(data) = sessions.get(conv_id) {
                if data.loaded {
                    return Ok(data.messages.clone());
                }
            }
        }
        // 缓存未命中或未加载 → 从磁盘读
        let messages = self.read_messages_from_disk(conv_id)?;
        let mut sessions = self.sessions.write().await;
        if let Some(data) = sessions.get_mut(conv_id) {
            data.messages = messages.clone();
            data.loaded = true;
        } else {
            // 会话不在缓存，尝试加载 meta
            let meta = self.read_meta_from_disk(conv_id)?;
            sessions.insert(
                conv_id.to_string(),
                ConversationData {
                    meta,
                    messages: messages.clone(),
                    loaded: true,
                },
            );
        }
        Ok(messages)
    }

    /// 懒加载分页：返回最新 N 条 + 总数 + has_more。
    ///
    /// - `limit: None` → 默认 50
    /// - `before: None` → 返回最新 N 条
    /// - `before: Some(i)` → 返回第 i 条之前的 N 条（即第 max(0, i-N) .. i 条）
    pub async fn load_messages_paged(
        &self,
        conv_id: &str,
        limit: Option<usize>,
        before: Option<usize>,
    ) -> Result<SessionMessagesPage, String> {
        validate_conversation_id(conv_id)?;
        let limit = limit.unwrap_or(DEFAULT_PAGE_LIMIT);
        let messages = self.load_messages(conv_id).await?;
        let total = messages.len();

        // before 是"第 N 条之前"的游标（1-indexed 位置）
        // None → 从最新开始（end）
        let end = before.unwrap_or(total);
        let start = end.saturating_sub(limit);

        let page: Vec<ChatMessage> = if start < end && end <= total {
            messages[start..end].to_vec()
        } else if end > total {
            // 游标越界，返回空页
            vec![]
        } else {
            vec![]
        };

        let has_more = start > 0;

        Ok(SessionMessagesPage {
            messages: page,
            total,
            has_more,
        })
    }

    /// 追加消息（append 内存 + append messages.jsonl + 更新 meta.updated_at/message_count）。
    ///
    /// 写穿一致性：先写磁盘成功，再更新内存。
    pub async fn append_message(
        &self,
        conv_id: &str,
        msg: ChatMessage,
    ) -> Result<(), String> {
        validate_conversation_id(conv_id)?;

        // 1. append messages.jsonl
        let msg_path = self.messages_path(conv_id)?;
        // 确保会话目录存在
        if let Some(parent) = msg_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("create session dir: {e}"))?;
            }
        }
        let line = serde_json::to_string(&msg)
            .map_err(|e| format!("serialize message: {e}"))?;
        {
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&msg_path)
                .map_err(|e| format!("open messages.jsonl: {e}"))?;
            writeln!(file, "{line}").map_err(|e| format!("write messages.jsonl: {e}"))?;
        }

        // 2. 更新内存缓存（更新 messages + meta.updated_at + meta.message_count）
        let now = msg.created_at;
        let mut sessions = self.sessions.write().await;
        let data = sessions
            .get_mut(conv_id)
            .ok_or_else(|| format!("session not in cache: {conv_id}"))?;
        data.messages.push(msg);
        data.meta.message_count = data.meta.message_count.saturating_add(1);
        if now > data.meta.updated_at {
            data.meta.updated_at = now;
        }

        // 3. atomic_write meta.json（updated_at / message_count 变了）
        let meta_json = serde_json::to_string_pretty(&data.meta)
            .map_err(|e| format!("serialize meta: {e}"))?;
        let meta_path = self.meta_path(conv_id)?;
        // 注意：这里不用 await，atomic_write 是同步函数
        drop(sessions);

        atomic_write(&meta_path, &meta_json, "meta")?;
        self.rewrite_index().await?;

        Ok(())
    }

    /// 更新 meta（title/pinned）—— atomic_write meta.json + 更新 index.json。
    pub async fn update_meta(
        &self,
        conv_id: &str,
        title: Option<&str>,
        pinned: Option<bool>,
    ) -> Result<Conversation, String> {
        validate_conversation_id(conv_id)?;

        let mut sessions = self.sessions.write().await;
        let data = sessions
            .get_mut(conv_id)
            .ok_or_else(|| format!("session not found: {conv_id}"))?;
        if let Some(t) = title {
            data.meta.title = t.to_string();
        }
        if let Some(p) = pinned {
            data.meta.pinned = p;
        }
        let updated = data.meta.clone();
        let meta_json = serde_json::to_string_pretty(&updated)
            .map_err(|e| format!("serialize meta: {e}"))?;
        drop(sessions);

        let meta_path = self.meta_path(conv_id)?;
        atomic_write(&meta_path, &meta_json, "meta")?;
        self.rewrite_index().await?;

        Ok(updated)
    }

    /// 删除会话（删目录 + 从缓存移除 + 更新 index.json）。
    pub async fn delete_session(&self, conv_id: &str) -> Result<(), String> {
        validate_conversation_id(conv_id)?;
        let dir = self.session_dir(conv_id)?;
        if dir.exists() {
            fs::remove_dir_all(&dir)
                .map_err(|e| format!("delete session dir: {e}"))?;
        }
        {
            let mut sessions = self.sessions.write().await;
            sessions.remove(conv_id);
        }
        self.rewrite_index().await?;
        Ok(())
    }

    /// 搜索会话（按 title 模糊匹配，大小写不敏感）。
    pub async fn search_sessions(
        &self,
        query: &str,
        limit: Option<usize>,
    ) -> Result<Vec<ConversationListItem>, String> {
        let limit = limit.unwrap_or(DEFAULT_PAGE_LIMIT);
        let query_lower = query.to_lowercase();
        let sessions = self.sessions.read().await;
        let mut results: Vec<ConversationListItem> = sessions
            .values()
            .filter(|d| d.meta.title.to_lowercase().contains(&query_lower))
            .map(|d| ConversationListItem {
                id: d.meta.id.clone(),
                title: d.meta.title.clone(),
                preview: d
                    .messages
                    .last()
                    .map(|m| {
                        m.content
                            .as_deref()
                            .unwrap_or("")
                            .chars()
                            .take(100)
                            .collect()
                    })
                    .unwrap_or_default(),
                created_at: d.meta.created_at,
                updated_at: d.meta.updated_at,
                pinned: d.meta.pinned,
                message_count: d.meta.message_count,
            })
            .collect();
        // 排序：pinned desc, updatedAt desc
        results.sort_by(|a, b| {
            if a.pinned != b.pinned {
                return b.pinned.cmp(&a.pinned);
            }
            b.updated_at.cmp(&a.updated_at)
        });
        results.truncate(limit);
        Ok(results)
    }

    /// 关闭时 flush（写穿模式下通常 no-op，但保留接口）。
    pub async fn flush_all(&self) -> Result<(), String> {
        // 写穿模式下内存与磁盘已同步，无需额外操作
        Ok(())
    }

    // -----------------------------------------------------------------------
    // 内部 helpers
    // -----------------------------------------------------------------------

    /// 全量重写 index.json（每次 meta 变更后调用）。
    async fn rewrite_index(&self) -> Result<(), String> {
        let sessions = self.sessions.read().await;
        let mut items: Vec<ConversationListItem> = sessions
            .values()
            .map(|d| ConversationListItem {
                id: d.meta.id.clone(),
                title: d.meta.title.clone(),
                preview: String::new(), // index.json 不存 preview（需读 messages）
                created_at: d.meta.created_at,
                updated_at: d.meta.updated_at,
                pinned: d.meta.pinned,
                message_count: d.meta.message_count,
            })
            .collect();
        drop(sessions);

        // 排序：pinned desc, updatedAt desc
        items.sort_by(|a, b| {
            if a.pinned != b.pinned {
                return b.pinned.cmp(&a.pinned);
            }
            b.updated_at.cmp(&a.updated_at)
        });

        let json = serde_json::to_string_pretty(&items)
            .map_err(|e| format!("serialize index: {e}"))?;
        let path = self.base_dir.join("index.json");
        atomic_write(&path, &json, "index")
    }

    /// 从磁盘读 messages.jsonl（逐行解析，损坏行跳过 + warn）。
    fn read_messages_from_disk(&self, conv_id: &str) -> Result<Vec<ChatMessage>, String> {
        let path = self.messages_path(conv_id)?;
        if !path.exists() {
            return Ok(vec![]);
        }
        let file = fs::File::open(&path).map_err(|e| format!("open messages.jsonl: {e}"))?;
        let reader = BufReader::new(file);
        let mut messages = vec![];
        for (idx, line) in reader.lines().enumerate() {
            let line = match line {
                Ok(l) => l,
                Err(e) => {
                    tracing::warn!("read_messages: skip line {}: IO error: {e}", idx);
                    continue;
                }
            };
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<ChatMessage>(&line) {
                Ok(msg) => messages.push(msg),
                Err(e) => {
                    tracing::warn!(
                        "read_messages: skip line {} (corrupted): {e} | line: {}",
                        idx,
                        &line[..line.len().min(200)]
                    );
                }
            }
        }
        Ok(messages)
    }

    /// 从磁盘读单个 meta.json（缓存未命中时用）。
    fn read_meta_from_disk(&self, conv_id: &str) -> Result<Conversation, String> {
        let path = self.meta_path(conv_id)?;
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("read meta.json: {e}"))?;
        serde_json::from_str(&content).map_err(|e| format!("parse meta.json: {e}"))
    }

    // -----------------------------------------------------------------------
    // 文件路径 helpers（内部用，转发到 storage 模块）
    // -----------------------------------------------------------------------

    fn meta_path(&self, conv_id: &str) -> Result<PathBuf, String> {
        Ok(self.base_dir.join(conv_id).join("meta.json"))
    }

    fn messages_path(&self, conv_id: &str) -> Result<PathBuf, String> {
        Ok(self.base_dir.join(conv_id).join("messages.jsonl"))
    }

    fn session_dir(&self, conv_id: &str) -> Result<PathBuf, String> {
        validate_conversation_id(conv_id)?;
        Ok(self.base_dir.join(conv_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// 构造一个用临时目录的 SessionStore。
    fn make_store(dir: &TempDir) -> SessionStore {
        SessionStore::new(dir.path().join("sessions"))
    }

    #[tokio::test]
    async fn create_session_writes_meta_and_index() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        store.load_index().await.unwrap();

        let conv = store.create_session().await.unwrap();
        assert!(conv.id.starts_with("conv_"));
        assert_eq!(conv.title, "New Session");
        assert_eq!(conv.message_count, 0);
        assert!(!conv.pinned);

        // meta.json 存在
        let meta_path = dir.path().join("sessions").join(&conv.id).join("meta.json");
        assert!(meta_path.exists());
        // index.json 存在
        let index_path = dir.path().join("sessions").join("index.json");
        assert!(index_path.exists());
    }

    #[tokio::test]
    async fn load_index_from_empty_dir() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        let items = store.load_index().await.unwrap();
        assert!(items.is_empty());
    }

    #[tokio::test]
    async fn load_index_from_existing_sessions() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        store.load_index().await.unwrap();
        let conv1 = store.create_session().await.unwrap();
        let conv2 = store.create_session().await.unwrap();

        // 新建一个 store（模拟重启）
        let store2 = make_store(&dir);
        let items = store2.load_index().await.unwrap();
        assert_eq!(items.len(), 2);
        let ids: Vec<_> = items.iter().map(|i| i.id.clone()).collect();
        assert!(ids.contains(&conv1.id));
        assert!(ids.contains(&conv2.id));
    }

    #[tokio::test]
    async fn append_message_persists_to_jsonl() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        store.load_index().await.unwrap();
        let conv = store.create_session().await.unwrap();

        let msg = ChatMessage::user("msg_1", "hello", 1720000000000);
        store.append_message(&conv.id, msg).await.unwrap();

        // 验证磁盘
        let messages_path = dir
            .path()
            .join("sessions")
            .join(&conv.id)
            .join("messages.jsonl");
        let content = fs::read_to_string(&messages_path).unwrap();
        assert!(content.contains("\"hello\""));
        assert!(content.contains("\"msg_1\""));
    }

    #[tokio::test]
    async fn append_message_updates_meta_count_and_updated_at() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        store.load_index().await.unwrap();
        let conv = store.create_session().await.unwrap();
        let original_updated = conv.updated_at;

        let msg = ChatMessage::user("msg_1", "hello", original_updated + 1000);
        store.append_message(&conv.id, msg).await.unwrap();

        let meta = store.get_meta(&conv.id).await.unwrap();
        assert_eq!(meta.message_count, 1);
        assert_eq!(meta.updated_at, original_updated + 1000);
    }

    #[tokio::test]
    async fn load_messages_returns_all_messages() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        store.load_index().await.unwrap();
        let conv = store.create_session().await.unwrap();

        store
            .append_message(&conv.id, ChatMessage::user("msg_1", "first", 1000))
            .await
            .unwrap();
        store
            .append_message(&conv.id, ChatMessage::user("msg_2", "second", 2000))
            .await
            .unwrap();

        // 新 store 模拟重启
        let store2 = make_store(&dir);
        store2.load_index().await.unwrap();
        let messages = store2.load_messages(&conv.id).await.unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].content.as_deref(), Some("first"));
        assert_eq!(messages[1].content.as_deref(), Some("second"));
    }

    #[tokio::test]
    async fn load_messages_paged_returns_latest_n() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        store.load_index().await.unwrap();
        let conv = store.create_session().await.unwrap();

        // 写 60 条消息
        for i in 0..60 {
            store
                .append_message(
                    &conv.id,
                    ChatMessage::user(format!("msg_{i}"), format!("content_{i}"), i as i64),
                )
                .await
                .unwrap();
        }

        // 默认 limit=50, before=None → 返回最新 50 条（第 10-59 条）
        let page = store
            .load_messages_paged(&conv.id, None, None)
            .await
            .unwrap();
        assert_eq!(page.messages.len(), 50);
        assert_eq!(page.total, 60);
        assert!(page.has_more);
        assert_eq!(page.messages[0].id, "msg_10");
        assert_eq!(page.messages[49].id, "msg_59");

        // before=10 → 返回第 0-9 条
        let page2 = store
            .load_messages_paged(&conv.id, Some(50), Some(10))
            .await
            .unwrap();
        assert_eq!(page2.messages.len(), 10);
        assert!(!page2.has_more);
        assert_eq!(page2.messages[0].id, "msg_0");
        assert_eq!(page2.messages[9].id, "msg_9");
    }

    #[tokio::test]
    async fn update_meta_renames_title() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        store.load_index().await.unwrap();
        let conv = store.create_session().await.unwrap();

        let updated = store
            .update_meta(&conv.id, Some("新标题"), None)
            .await
            .unwrap();
        assert_eq!(updated.title, "新标题");

        // 重启后验证
        let store2 = make_store(&dir);
        store2.load_index().await.unwrap();
        let meta = store2.get_meta(&conv.id).await.unwrap();
        assert_eq!(meta.title, "新标题");
    }

    #[tokio::test]
    async fn update_meta_toggles_pinned() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        store.load_index().await.unwrap();
        let conv = store.create_session().await.unwrap();
        assert!(!conv.pinned);

        let updated = store
            .update_meta(&conv.id, None, Some(true))
            .await
            .unwrap();
        assert!(updated.pinned);

        let updated2 = store
            .update_meta(&conv.id, None, Some(false))
            .await
            .unwrap();
        assert!(!updated2.pinned);
    }

    #[tokio::test]
    async fn delete_session_removes_directory_and_cache() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        store.load_index().await.unwrap();
        let conv = store.create_session().await.unwrap();
        let session_dir = dir.path().join("sessions").join(&conv.id);
        assert!(session_dir.exists());

        store.delete_session(&conv.id).await.unwrap();

        assert!(!session_dir.exists());
        let meta_result = store.get_meta(&conv.id).await;
        assert!(meta_result.is_err());
    }

    #[tokio::test]
    async fn search_sessions_filters_by_title() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        store.load_index().await.unwrap();

        let mut conv1 = store.create_session().await.unwrap();
        conv1 = store
            .update_meta(&conv1.id, Some("fix auth bug"), None)
            .await
            .unwrap();
        let conv2 = store.create_session().await.unwrap();
        let _ = store
            .update_meta(&conv2.id, Some("add feature X"), None)
            .await
            .unwrap();

        let results = store.search_sessions("auth", None).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, conv1.id);

        let results2 = store.search_sessions("feature", None).await.unwrap();
        assert_eq!(results2.len(), 1);

        // 搜索 "add" 应该匹配 conv2（"add feature X"）
        let results3 = store.search_sessions("add", None).await.unwrap();
        assert_eq!(results3.len(), 1);
        assert_eq!(results3[0].id, conv2.id);

        let results4 = store.search_sessions("nonexistent", None).await.unwrap();
        assert!(results4.is_empty());
    }

    #[tokio::test]
    async fn load_messages_skips_corrupted_jsonl_line() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        store.load_index().await.unwrap();
        let conv = store.create_session().await.unwrap();

        // 手动写一个 messages.jsonl，第二行损坏
        let messages_path = dir
            .path()
            .join("sessions")
            .join(&conv.id)
            .join("messages.jsonl");
        let valid_line1 =
            r#"{"id":"msg_1","role":"user","content":"hello","createdAt":1000}"#;
        let corrupted_line = r#"{"id":"msg_2","role":"user","content":INVALID}"#;
        let valid_line2 =
            r#"{"id":"msg_3","role":"user","content":"world","createdAt":2000}"#;
        fs::write(
            &messages_path,
            format!("{valid_line1}\n{corrupted_line}\n{valid_line2}\n"),
        )
        .unwrap();

        // 新 store 模拟重启
        let store2 = make_store(&dir);
        store2.load_index().await.unwrap();
        let messages = store2.load_messages(&conv.id).await.unwrap();

        // 损坏行跳过，保留 2 条有效消息
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].id, "msg_1");
        assert_eq!(messages[1].id, "msg_3");
    }
}
