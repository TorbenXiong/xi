use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

pub const UNGROUPED_NAME: &str = "未分组";

#[derive(Debug, Clone)]
pub struct GroupSummary {
    pub id: String,
    pub name: String,
    pub is_system: bool,
    pub conversation_count: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct ConversationSummary {
    pub id: String,
    pub group_id: String,
    pub title: String,
    pub model: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct StoredMessage {
    pub sequence: i64,
    pub role: String,
    pub content: String,
    pub status: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RunHandle {
    pub id: String,
    pub assistant_id: String,
}

pub struct Store {
    connection: Connection,
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("无法创建数据库目录 {}", parent.display()))?;
        }

        let connection =
            Connection::open(path).with_context(|| format!("无法打开数据库 {}", path.display()))?;
        connection
            .pragma_update(None, "foreign_keys", true)
            .context("无法启用 SQLite 外键约束")?;
        connection
            .execute_batch("PRAGMA journal_mode = WAL; PRAGMA busy_timeout = 5000;")
            .context("无法配置 SQLite")?;

        let mut store = Self { connection };
        store.migrate()?;
        store.ensure_ungrouped_group()?;
        Ok(store)
    }

    fn migrate(&mut self) -> Result<()> {
        let version: i64 = self
            .connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .context("无法读取数据库版本")?;
        if version >= 1 {
            return Ok(());
        }

        self.connection
            .execute_batch(
                "
                BEGIN;
                CREATE TABLE IF NOT EXISTS conversation_groups (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    sort_order INTEGER NOT NULL DEFAULT 0,
                    is_system INTEGER NOT NULL DEFAULT 0,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL,
                    archived_at INTEGER
                );
                CREATE UNIQUE INDEX IF NOT EXISTS idx_groups_active_name
                    ON conversation_groups(name COLLATE NOCASE)
                    WHERE archived_at IS NULL;

                CREATE TABLE IF NOT EXISTS conversations (
                    id TEXT PRIMARY KEY,
                    group_id TEXT NOT NULL,
                    title TEXT NOT NULL,
                    model TEXT NOT NULL,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL,
                    archived_at INTEGER,
                    FOREIGN KEY (group_id) REFERENCES conversation_groups(id)
                );
                CREATE INDEX IF NOT EXISTS idx_conversations_group_updated
                    ON conversations(group_id, archived_at, updated_at DESC);

                CREATE TABLE IF NOT EXISTS messages (
                    id TEXT PRIMARY KEY,
                    conversation_id TEXT NOT NULL,
                    sequence INTEGER NOT NULL,
                    role TEXT NOT NULL CHECK (role IN ('system', 'user', 'assistant', 'tool')),
                    content TEXT NOT NULL,
                    status TEXT NOT NULL CHECK (
                        status IN ('pending', 'streaming', 'complete', 'failed', 'interrupted')
                    ),
                    error TEXT,
                    created_at INTEGER NOT NULL,
                    completed_at INTEGER,
                    FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE,
                    UNIQUE (conversation_id, sequence)
                );
                CREATE INDEX IF NOT EXISTS idx_messages_conversation_sequence
                    ON messages(conversation_id, sequence);

                CREATE TABLE IF NOT EXISTS runs (
                    id TEXT PRIMARY KEY,
                    conversation_id TEXT NOT NULL,
                    user_message_id TEXT NOT NULL,
                    assistant_id TEXT,
                    model TEXT NOT NULL,
                    status TEXT NOT NULL CHECK (
                        status IN ('queued', 'running', 'completed', 'failed', 'interrupted')
                    ),
                    started_at INTEGER NOT NULL,
                    finished_at INTEGER,
                    error TEXT,
                    usage_json TEXT,
                    FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE,
                    FOREIGN KEY (user_message_id) REFERENCES messages(id),
                    FOREIGN KEY (assistant_id) REFERENCES messages(id)
                );
                CREATE INDEX IF NOT EXISTS idx_runs_conversation_started
                    ON runs(conversation_id, started_at DESC);

                CREATE TABLE IF NOT EXISTS app_state (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL
                );
                PRAGMA user_version = 1;
                COMMIT;
                ",
            )
            .context("初始化数据库结构失败")?;
        Ok(())
    }

    fn ensure_ungrouped_group(&self) -> Result<String> {
        if let Some(id) = self
            .connection
            .query_row(
                "SELECT id FROM conversation_groups WHERE is_system = 1 LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .context("查询未分组失败")?
        {
            return Ok(id);
        }

        let id = new_id();
        let now = now();
        self.connection
            .execute(
                "INSERT INTO conversation_groups
                    (id, name, sort_order, is_system, created_at, updated_at)
                 VALUES (?1, ?2, 0, 1, ?3, ?3)",
                params![id, UNGROUPED_NAME, now],
            )
            .context("创建未分组失败")?;
        Ok(id)
    }

    pub fn ungrouped_id(&self) -> Result<String> {
        self.connection
            .query_row(
                "SELECT id FROM conversation_groups WHERE is_system = 1 LIMIT 1",
                [],
                |row| row.get(0),
            )
            .context("读取未分组失败")
    }

    pub fn get_state(&self, key: &str) -> Result<Option<String>> {
        self.connection
            .query_row("SELECT value FROM app_state WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()
            .context("读取应用状态失败")
    }

    pub fn set_state(&self, key: &str, value: &str) -> Result<()> {
        self.connection
            .execute(
                "INSERT INTO app_state(key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .context("保存应用状态失败")?;
        Ok(())
    }

    pub fn list_groups(&self, include_archived: bool) -> Result<Vec<GroupSummary>> {
        let filter = if include_archived {
            ""
        } else {
            "WHERE g.archived_at IS NULL"
        };
        let sql = format!(
            "SELECT g.id, g.name, g.is_system, g.updated_at,
                    COUNT(CASE WHEN c.id IS NOT NULL AND c.archived_at IS NULL THEN 1 END)
             FROM conversation_groups g
             LEFT JOIN conversations c ON c.group_id = g.id
             {filter}
             GROUP BY g.id
             ORDER BY g.sort_order, g.updated_at DESC, g.name COLLATE NOCASE"
        );
        let mut statement = self.connection.prepare(&sql).context("查询分组失败")?;
        let rows = statement.query_map([], |row| {
            Ok(GroupSummary {
                id: row.get(0)?,
                name: row.get(1)?,
                is_system: row.get::<_, i64>(2)? != 0,
                updated_at: row.get(3)?,
                conversation_count: row.get(4)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("读取分组失败")
    }

    pub fn create_group(&self, name: &str) -> Result<GroupSummary> {
        let name = normalized_name(name)?;
        let id = new_id();
        let now = now();
        self.connection
            .execute(
                "INSERT INTO conversation_groups
                    (id, name, sort_order, is_system, created_at, updated_at)
                 VALUES (?1, ?2, 0, 0, ?3, ?3)",
                params![id, name, now],
            )
            .with_context(|| format!("创建分组失败，名称可能已存在：{name}"))?;
        self.group_by_id(&id)
    }

    pub fn rename_group(&self, id: &str, name: &str) -> Result<()> {
        let name = normalized_name(name)?;
        let is_system: bool = self
            .connection
            .query_row(
                "SELECT is_system FROM conversation_groups WHERE id = ?1 AND archived_at IS NULL",
                [id],
                |row| Ok(row.get::<_, i64>(0)? != 0),
            )
            .optional()
            .context("查询分组失败")?
            .ok_or_else(|| anyhow!("分组不存在或已归档"))?;
        if is_system {
            bail!("不能重命名系统分组");
        }
        self.connection
            .execute(
                "UPDATE conversation_groups SET name = ?1, updated_at = ?2 WHERE id = ?3",
                params![name, now(), id],
            )
            .context("重命名分组失败")?;
        Ok(())
    }

    pub fn archive_group(&self, id: &str) -> Result<()> {
        self.ensure_mutable_group(id)?;
        self.connection
            .execute(
                "UPDATE conversation_groups SET archived_at = ?1, updated_at = ?1 WHERE id = ?2",
                params![now(), id],
            )
            .context("归档分组失败")?;
        Ok(())
    }

    pub fn delete_group(&mut self, id: &str) -> Result<()> {
        let ungrouped_id = self.ungrouped_id()?;
        self.ensure_mutable_group(id)?;
        let transaction = self
            .connection
            .transaction()
            .context("开始删除分组事务失败")?;
        let timestamp = now();
        transaction
            .execute(
                "UPDATE conversations SET group_id = ?1, updated_at = ?2 WHERE group_id = ?3",
                params![ungrouped_id, timestamp, id],
            )
            .context("移动分组会话失败")?;
        transaction
            .execute(
                "UPDATE conversation_groups SET archived_at = ?1, updated_at = ?1 WHERE id = ?2",
                params![timestamp, id],
            )
            .context("删除分组失败")?;
        transaction.commit().context("提交删除分组事务失败")?;
        Ok(())
    }

    pub fn list_conversations(&self, group_id: Option<&str>) -> Result<Vec<ConversationSummary>> {
        let mut sql = String::from(
            "SELECT id, group_id, title, model, updated_at
             FROM conversations WHERE archived_at IS NULL",
        );
        if group_id.is_some() {
            sql.push_str(" AND group_id = ?1");
        }
        sql.push_str(" ORDER BY updated_at DESC");
        let mut statement = self.connection.prepare(&sql).context("查询会话失败")?;
        let rows = if let Some(group_id) = group_id {
            statement.query_map([group_id], conversation_from_row)?
        } else {
            statement.query_map([], conversation_from_row)?
        };
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("读取会话失败")
    }

    pub fn create_conversation(
        &self,
        group_id: &str,
        title: &str,
        model: &str,
    ) -> Result<ConversationSummary> {
        self.ensure_active_group(group_id)?;
        let id = new_id();
        let now = now();
        let title = if title.trim().is_empty() {
            "新会话"
        } else {
            title.trim()
        };
        self.connection
            .execute(
                "INSERT INTO conversations
                    (id, group_id, title, model, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                params![id, group_id, title, model, now],
            )
            .context("创建会话失败")?;
        self.conversation_by_id(&id)
    }

    pub fn rename_conversation(&self, id: &str, title: &str) -> Result<()> {
        let title = normalized_name(title)?;
        self.connection
            .execute(
                "UPDATE conversations SET title = ?1, updated_at = ?2
                 WHERE id = ?3 AND archived_at IS NULL",
                params![title, now(), id],
            )
            .context("重命名会话失败")?;
        Ok(())
    }

    pub fn move_conversation(&self, id: &str, group_id: &str) -> Result<()> {
        self.ensure_active_group(group_id)?;
        let changed = self.connection.execute(
            "UPDATE conversations SET group_id = ?1, updated_at = ?2
                 WHERE id = ?3 AND archived_at IS NULL",
            params![group_id, now(), id],
        )?;
        if changed == 0 {
            bail!("会话不存在或已归档");
        }
        Ok(())
    }

    pub fn archive_conversation(&self, id: &str) -> Result<()> {
        let changed = self.connection.execute(
            "UPDATE conversations SET archived_at = ?1, updated_at = ?1
                 WHERE id = ?2 AND archived_at IS NULL",
            params![now(), id],
        )?;
        if changed == 0 {
            bail!("会话不存在或已归档");
        }
        Ok(())
    }

    pub fn clear_conversation(&mut self, id: &str) -> Result<()> {
        let transaction = self
            .connection
            .transaction()
            .context("开始清空会话事务失败")?;
        let exists: Option<String> = transaction
            .query_row(
                "SELECT id FROM conversations WHERE id = ?1 AND archived_at IS NULL",
                [id],
                |row| row.get(0),
            )
            .optional()?;
        if exists.is_none() {
            bail!("会话不存在或已归档");
        }
        transaction
            .execute("DELETE FROM runs WHERE conversation_id = ?1", [id])
            .context("删除会话运行记录失败")?;
        transaction
            .execute("DELETE FROM messages WHERE conversation_id = ?1", [id])
            .context("删除会话消息失败")?;
        transaction
            .execute(
                "UPDATE conversations SET updated_at = ?1 WHERE id = ?2",
                params![now(), id],
            )
            .context("更新会话时间失败")?;
        transaction.commit().context("提交清空会话事务失败")?;
        Ok(())
    }

    pub fn search(&self, keyword: &str) -> Result<Vec<ConversationSummary>> {
        let keyword = keyword.trim();
        if keyword.is_empty() {
            bail!("搜索关键词不能为空");
        }
        let pattern = format!("%{keyword}%");
        let mut statement = self.connection.prepare(
            "SELECT DISTINCT c.id, c.group_id, c.title, c.model, c.updated_at
             FROM conversations c
             JOIN conversation_groups g ON g.id = c.group_id
             LEFT JOIN messages m ON m.conversation_id = c.id
             WHERE c.archived_at IS NULL AND g.archived_at IS NULL
               AND (c.title LIKE ?1 OR m.content LIKE ?1)
             ORDER BY c.updated_at DESC",
        )?;
        let rows = statement.query_map([pattern], conversation_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("读取搜索结果失败")
    }

    pub fn load_messages(
        &self,
        conversation_id: &str,
        complete_only: bool,
    ) -> Result<Vec<StoredMessage>> {
        let status_filter = if complete_only {
            " AND status = 'complete'"
        } else {
            ""
        };
        let sql = format!(
            "SELECT sequence, role, content, status, error
             FROM messages WHERE conversation_id = ?1{status_filter} ORDER BY sequence"
        );
        let mut statement = self.connection.prepare(&sql).context("查询消息失败")?;
        let rows = statement.query_map([conversation_id], |row| {
            Ok(StoredMessage {
                sequence: row.get(0)?,
                role: row.get(1)?,
                content: row.get(2)?,
                status: row.get(3)?,
                error: row.get(4)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("读取消息失败")
    }

    pub fn start_run(
        &mut self,
        conversation_id: &str,
        user_content: &str,
        model: &str,
    ) -> Result<RunHandle> {
        let transaction = self
            .connection
            .transaction()
            .context("开始创建运行事务失败")?;
        let exists: Option<String> = transaction
            .query_row(
                "SELECT id FROM conversations WHERE id = ?1 AND archived_at IS NULL",
                [conversation_id],
                |row| row.get(0),
            )
            .optional()?;
        if exists.is_none() {
            bail!("会话不存在或已归档");
        }

        let next_sequence: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(sequence), -1) + 1 FROM messages WHERE conversation_id = ?1",
            [conversation_id],
            |row| row.get(0),
        )?;
        let user_id = new_id();
        let assistant_id = new_id();
        let run_id = new_id();
        let timestamp = now();
        transaction.execute(
            "INSERT INTO messages
                (id, conversation_id, sequence, role, content, status, created_at, completed_at)
             VALUES (?1, ?2, ?3, 'user', ?4, 'complete', ?5, ?5)",
            params![
                user_id,
                conversation_id,
                next_sequence,
                user_content,
                timestamp
            ],
        )?;
        transaction.execute(
            "INSERT INTO messages
                (id, conversation_id, sequence, role, content, status, created_at)
             VALUES (?1, ?2, ?3, 'assistant', '', 'streaming', ?4)",
            params![assistant_id, conversation_id, next_sequence + 1, timestamp],
        )?;
        transaction.execute(
            "INSERT INTO runs
                (id, conversation_id, user_message_id, assistant_id, model, status, started_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'running', ?6)",
            params![
                run_id,
                conversation_id,
                user_id,
                assistant_id,
                model,
                timestamp
            ],
        )?;
        transaction.execute(
            "UPDATE conversations SET updated_at = ?1 WHERE id = ?2",
            params![timestamp, conversation_id],
        )?;
        transaction.commit().context("提交创建运行事务失败")?;
        Ok(RunHandle {
            id: run_id,
            assistant_id,
        })
    }

    pub fn complete_run(&mut self, run: &RunHandle, content: &str) -> Result<()> {
        self.finish_run(run, "complete", content, None)
    }

    pub fn fail_run(&mut self, run: &RunHandle, error: &str) -> Result<()> {
        self.finish_run(run, "failed", "", Some(error))
    }

    pub fn interrupt_run(
        &mut self,
        run: &RunHandle,
        partial_content: &str,
        error: &str,
    ) -> Result<()> {
        self.finish_run(run, "interrupted", partial_content, Some(error))
    }

    fn finish_run(
        &mut self,
        run: &RunHandle,
        status: &str,
        content: &str,
        error: Option<&str>,
    ) -> Result<()> {
        let transaction = self
            .connection
            .transaction()
            .context("开始完成运行事务失败")?;
        let timestamp = now();
        let run_status = if status == "complete" {
            "completed"
        } else {
            status
        };
        transaction.execute(
            "UPDATE messages SET content = ?1, status = ?2, error = ?3, completed_at = ?4
             WHERE id = ?5",
            params![content, status, error, timestamp, run.assistant_id],
        )?;
        transaction.execute(
            "UPDATE runs SET status = ?1, error = ?2, finished_at = ?3
             WHERE id = ?4",
            params![run_status, error, timestamp, run.id],
        )?;
        transaction.commit().context("提交完成运行事务失败")?;
        Ok(())
    }

    pub fn mark_stale_runs_interrupted(&mut self) -> Result<usize> {
        let timestamp = now();
        let transaction = self
            .connection
            .transaction()
            .context("开始恢复运行状态事务失败")?;
        let count = transaction.execute(
            "UPDATE messages SET status = 'interrupted', error = '程序上次退出时请求未完成', completed_at = ?1
             WHERE status IN ('pending', 'streaming')",
            [timestamp],
        )?;
        transaction.execute(
            "UPDATE runs SET status = 'interrupted', error = '程序上次退出时请求未完成', finished_at = ?1
             WHERE status IN ('queued', 'running')",
            [timestamp],
        )?;
        transaction.commit().context("提交恢复运行状态事务失败")?;
        Ok(count)
    }

    pub fn group_by_id(&self, id: &str) -> Result<GroupSummary> {
        self.list_groups(true)?
            .into_iter()
            .find(|group| group.id == id)
            .ok_or_else(|| anyhow!("分组不存在：{id}"))
    }

    pub fn conversation_by_id(&self, id: &str) -> Result<ConversationSummary> {
        self.connection
            .query_row(
                "SELECT id, group_id, title, model, updated_at
                 FROM conversations WHERE id = ?1 AND archived_at IS NULL",
                [id],
                conversation_from_row,
            )
            .optional()
            .context("查询会话失败")?
            .ok_or_else(|| anyhow!("会话不存在或已归档：{id}"))
    }

    pub fn resolve_group(&self, value: &str) -> Result<GroupSummary> {
        let groups = self.list_groups(false)?;
        let normalized = value.trim();
        groups
            .into_iter()
            .find(|group| group.id == normalized || group.name.eq_ignore_ascii_case(normalized))
            .ok_or_else(|| anyhow!("找不到分组：{normalized}"))
    }

    pub fn resolve_conversation(&self, value: &str) -> Result<ConversationSummary> {
        let conversations = self.list_conversations(None)?;
        let normalized = value.trim();
        conversations
            .iter()
            .find(|conversation| conversation.id == normalized)
            .cloned()
            .or_else(|| {
                conversations
                    .iter()
                    .find(|conversation| conversation.title.eq_ignore_ascii_case(normalized))
                    .cloned()
            })
            .ok_or_else(|| anyhow!("找不到会话：{normalized}"))
    }

    fn ensure_active_group(&self, id: &str) -> Result<()> {
        if self
            .list_groups(false)?
            .into_iter()
            .any(|group| group.id == id)
        {
            return Ok(());
        }
        bail!("分组不存在或已归档")
    }

    fn ensure_mutable_group(&self, id: &str) -> Result<()> {
        let group = self.group_by_id(id)?;
        if group.is_system || !self.list_groups(false)?.iter().any(|item| item.id == id) {
            bail!("不能修改系统分组");
        }
        Ok(())
    }
}

fn conversation_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ConversationSummary> {
    Ok(ConversationSummary {
        id: row.get(0)?,
        group_id: row.get(1)?,
        title: row.get(2)?,
        model: row.get(3)?,
        updated_at: row.get(4)?,
    })
}

fn normalized_name(value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        bail!("名称不能为空");
    }
    Ok(value.to_owned())
}

fn new_id() -> String {
    Uuid::new_v4().to_string()
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub fn default_database_path() -> Result<PathBuf> {
    let root = if cfg!(target_os = "windows") {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow!("无法确定 Windows 本地数据目录"))?
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow!("无法确定 macOS 用户目录"))?
            .join("Library/Application Support")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
            })
            .ok_or_else(|| anyhow!("无法确定 Linux 数据目录"))?
    };
    Ok(root.join("xi").join("xi.db"))
}

#[cfg(test)]
mod tests {
    use super::{Store, UNGROUPED_NAME};

    #[test]
    fn persists_groups_conversations_and_messages() {
        let store = Store::open(":memory:").unwrap();
        let ungrouped = store.ungrouped_id().unwrap();
        assert_eq!(store.group_by_id(&ungrouped).unwrap().name, UNGROUPED_NAME);

        let group = store.create_group("项目 A").unwrap();
        let conversation = store
            .create_conversation(&group.id, "测试会话", "test-model")
            .unwrap();
        let mut store = store;
        let run = store
            .start_run(&conversation.id, "你好", "test-model")
            .unwrap();
        assert_eq!(
            store.load_messages(&conversation.id, true).unwrap().len(),
            1
        );
        store.complete_run(&run, "你好！").unwrap();
        let messages = store.load_messages(&conversation.id, true).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[1].content, "你好！");
    }

    #[test]
    fn deleting_group_moves_conversations_to_ungrouped() {
        let mut store = Store::open(":memory:").unwrap();
        let group = store.create_group("待删除").unwrap();
        let conversation = store
            .create_conversation(&group.id, "会话", "model")
            .unwrap();
        store.delete_group(&group.id).unwrap();
        assert_eq!(
            store.conversation_by_id(&conversation.id).unwrap().group_id,
            store.ungrouped_id().unwrap()
        );
    }

    #[test]
    fn preserves_interrupted_partial_response() {
        let mut store = Store::open(":memory:").unwrap();
        let group = store.create_group("运行状态").unwrap();
        let conversation = store
            .create_conversation(&group.id, "会话", "model")
            .unwrap();
        let run = store.start_run(&conversation.id, "问题", "model").unwrap();
        store
            .interrupt_run(&run, "已生成的一部分", "网络中断")
            .unwrap();

        let messages = store.load_messages(&conversation.id, false).unwrap();
        assert_eq!(messages[1].status, "interrupted");
        assert_eq!(messages[1].content, "已生成的一部分");
        assert_eq!(messages[1].error.as_deref(), Some("网络中断"));
    }
}
