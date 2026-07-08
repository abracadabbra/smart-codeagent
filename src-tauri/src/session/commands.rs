//! 会话管理 IPC 命令（design.md §7.1）。
//!
//! Round 4：纯新增，**暂不注册到 `invoke_handler`**（Round 5 一起注册）。
//! 命令通过 `State<'_, Arc<SessionStore>>` 取 store（Round 5 在 `lib.rs` setup 时 manage）。

use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::session::store::SessionStore;
use crate::session::types::{Conversation, ConversationListItem, SessionMessagesPage};

/// 创建新会话。生成 `conv_{uuid}`，写 meta.json + index.json。
#[tauri::command]
pub async fn create_session(store: State<'_, Arc<SessionStore>>) -> Result<Conversation, String> {
    store.create_session().await
}

/// 列出所有会话（按 pinned desc, updatedAt desc 排序）。
#[tauri::command]
pub async fn list_sessions(
    store: State<'_, Arc<SessionStore>>,
) -> Result<Vec<ConversationListItem>, String> {
    store.load_index().await
}

/// 获取单个会话元数据。
#[tauri::command]
pub async fn get_session(
    store: State<'_, Arc<SessionStore>>,
    conversation_id: String,
) -> Result<Conversation, String> {
    store.get_meta(&conversation_id).await
}

/// 懒加载分页获取会话消息。
///
/// - `limit`：每页条数，None → 默认 50
/// - `before`：游标，None → 从最新开始
#[tauri::command]
pub async fn get_session_messages(
    store: State<'_, Arc<SessionStore>>,
    conversation_id: String,
    limit: Option<usize>,
    before: Option<usize>,
) -> Result<SessionMessagesPage, String> {
    store
        .load_messages_paged(&conversation_id, limit, before)
        .await
}

/// 更新会话元数据（title / pinned）。传 None 的字段不更新。
#[tauri::command]
pub async fn update_session(
    store: State<'_, Arc<SessionStore>>,
    conversation_id: String,
    title: Option<String>,
    pinned: Option<bool>,
) -> Result<Conversation, String> {
    store
        .update_meta(&conversation_id, title.as_deref(), pinned)
        .await
}

/// 删除会话（删目录 + 从缓存移除 + 更新 index.json）。
#[tauri::command]
pub async fn delete_session(
    store: State<'_, Arc<SessionStore>>,
    conversation_id: String,
) -> Result<(), String> {
    store.delete_session(&conversation_id).await
}

/// 搜索会话（按 title 模糊匹配，大小写不敏感）。
///
/// - `limit`：返回上限，None → 默认 50
#[tauri::command]
pub async fn search_sessions(
    store: State<'_, Arc<SessionStore>>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<ConversationListItem>, String> {
    store.search_sessions(&query, limit).await
}

// 反引用 AppHandle 防止 unused warning（命令签名未来可能加 app 参数）
#[allow(dead_code)]
fn _unused(_: &AppHandle) {}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// 构造一个用临时目录的 SessionStore（不通过 Tauri manage，直接测内部逻辑）。
    fn make_store(dir: &TempDir) -> SessionStore {
        SessionStore::new(dir.path().join("sessions"))
    }

    #[tokio::test]
    async fn create_then_list_returns_one_session() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        store.load_index().await.unwrap();

        let conv = store.create_session().await.unwrap();
        let items = store.load_index().await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, conv.id);
    }

    #[tokio::test]
    async fn get_session_returns_meta() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        store.load_index().await.unwrap();
        let conv = store.create_session().await.unwrap();

        let meta = store.get_meta(&conv.id).await.unwrap();
        assert_eq!(meta.id, conv.id);
        assert_eq!(meta.title, "New Session");
    }

    #[tokio::test]
    async fn get_session_messages_returns_paged() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        store.load_index().await.unwrap();
        let conv = store.create_session().await.unwrap();

        for i in 0..10 {
            store
                .append_message(
                    &conv.id,
                    crate::session::types::ChatMessage::user(
                        format!("msg_{i}"),
                        format!("content_{i}"),
                        i as i64,
                    ),
                )
                .await
                .unwrap();
        }

        let page = store
            .load_messages_paged(&conv.id, Some(5), None)
            .await
            .unwrap();
        assert_eq!(page.messages.len(), 5);
        assert_eq!(page.total, 10);
        assert!(page.has_more);
    }

    #[tokio::test]
    async fn update_session_renames_title() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        store.load_index().await.unwrap();
        let conv = store.create_session().await.unwrap();

        let updated = store
            .update_meta(&conv.id, Some("new title"), None)
            .await
            .unwrap();
        assert_eq!(updated.title, "new title");
    }

    #[tokio::test]
    async fn delete_session_removes_it() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        store.load_index().await.unwrap();
        let conv = store.create_session().await.unwrap();

        store.delete_session(&conv.id).await.unwrap();
        assert!(store.get_meta(&conv.id).await.is_err());
    }

    #[tokio::test]
    async fn search_sessions_finds_by_title() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        store.load_index().await.unwrap();
        let conv = store.create_session().await.unwrap();
        store
            .update_meta(&conv.id, Some("fix auth bug"), None)
            .await
            .unwrap();

        let results = store.search_sessions("auth", None).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, conv.id);
    }
}
