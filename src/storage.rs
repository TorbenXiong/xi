use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use uuid::Uuid;

mod providers;

#[derive(Debug, Clone, Serialize)]
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
    pub directory: String,
    pub conversation_count: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConversationSummary {
    pub id: String,
    pub project_id: Option<String>,
    pub title: String,
    pub model: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
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
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)
                .with_context(|| format!("无法创建数据库目录 {}", parent.display()))?;
        }
        let connection =
            Connection::open(path).with_context(|| format!("无法打开数据库 {}", path.display()))?;
        connection.pragma_update(None, "foreign_keys", true)?;
        connection.execute_batch("PRAGMA journal_mode = WAL; PRAGMA busy_timeout = 5000;")?;
        let store = Self { connection };
        store.initialize()?;
        store.init_provider_tables()?;
        Ok(store)
    }

    fn initialize(&self) -> Result<()> {
        self.connection
            .execute_batch(
                "BEGIN;
             CREATE TABLE IF NOT EXISTS projects (
                 id TEXT PRIMARY KEY,
                 name TEXT NOT NULL,
                 directory TEXT NOT NULL UNIQUE,
                 created_at INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL,
                 archived_at INTEGER
             );
             CREATE TABLE IF NOT EXISTS conversations (
                 id TEXT PRIMARY KEY,
                 project_id TEXT,
                 title TEXT NOT NULL,
                 model TEXT NOT NULL,
                 created_at INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL,
                 archived_at INTEGER,
                 FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE SET NULL
             );
             CREATE INDEX IF NOT EXISTS idx_conversations_project_updated
                 ON conversations(project_id, archived_at, updated_at DESC);
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
             COMMIT;",
            )
            .context("初始化数据库结构失败")?;
        Ok(())
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
        self.connection.execute(
            "INSERT INTO app_state(key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key,value],
        )?;
        Ok(())
    }

    pub fn remove_state(&self, key: &str) -> Result<()> {
        self.connection
            .execute("DELETE FROM app_state WHERE key=?1", [key])?;
        Ok(())
    }

    pub fn list_projects(&self, include_archived: bool) -> Result<Vec<ProjectSummary>> {
        let filter = if include_archived {
            ""
        } else {
            "WHERE p.archived_at IS NULL"
        };
        let sql = format!(
            "SELECT p.id,p.name,p.directory,p.updated_at,COUNT(CASE WHEN c.archived_at IS NULL THEN c.id END)
             FROM projects p LEFT JOIN conversations c ON c.project_id=p.id {filter}
             GROUP BY p.id ORDER BY p.updated_at DESC,p.name COLLATE NOCASE,p.id"
        );
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map([], |row| {
            Ok(ProjectSummary {
                id: row.get(0)?,
                name: row.get(1)?,
                directory: row.get(2)?,
                updated_at: row.get(3)?,
                conversation_count: row.get(4)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("读取项目失败")
    }

    pub fn create_project(&self, directory: &str, name: &str) -> Result<ProjectSummary> {
        let path = Path::new(directory.trim());
        if !path.is_absolute() {
            bail!("项目目录必须是绝对路径");
        }
        let path = path.canonicalize().context("项目目录不存在或无法访问")?;
        if !path.is_dir() {
            bail!("项目路径必须是文件夹");
        }
        let directory = path
            .to_str()
            .context("项目目录包含无法表示的字符")?
            .to_owned();
        #[cfg(windows)]
        let directory = if let Some(path) = directory.strip_prefix(r"\\?\UNC\") {
            format!(r"\\{path}")
        } else {
            directory
                .strip_prefix(r"\\?\")
                .unwrap_or(&directory)
                .to_owned()
        };
        if let Some(project) = self.list_projects(true)?.into_iter().find(|project| {
            if cfg!(windows) {
                project.directory.eq_ignore_ascii_case(&directory)
            } else {
                project.directory == directory
            }
        }) {
            self.connection.execute(
                "UPDATE projects SET archived_at=NULL,updated_at=?1 WHERE id=?2",
                params![now(), project.id],
            )?;
            return self.project_by_id(&project.id);
        }
        let name = if name.trim().is_empty() {
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(&directory)
                .to_owned()
        } else {
            normalized_name(name)?
        };
        let id = new_id();
        self.connection.execute(
            "INSERT INTO projects(id,name,directory,created_at,updated_at) VALUES (?1,?2,?3,?4,?4)",
            params![id,name,directory,now()],
        ).context("创建项目失败")?;
        self.project_by_id(&id)
    }

    pub fn project_by_id(&self, id: &str) -> Result<ProjectSummary> {
        self.list_projects(false)?
            .into_iter()
            .find(|project| project.id == id)
            .context("项目不存在或已归档")
    }

    pub fn resolve_project(&self, value: &str) -> Result<ProjectSummary> {
        let projects = self.list_projects(false)?;
        if let Some(project) = projects
            .iter()
            .find(|project| project.id == value || project.directory == value)
        {
            return Ok(project.clone());
        }
        let matches = projects
            .into_iter()
            .filter(|project| project.name.eq_ignore_ascii_case(value.trim()))
            .collect::<Vec<_>>();
        if matches.len() > 1 {
            bail!("存在多个同名项目，请使用项目 ID 或完整目录");
        }
        matches.into_iter().next().context("项目不存在或已归档")
    }

    pub fn rename_project(&self, id: &str, name: &str) -> Result<()> {
        self.project_by_id(id)?;
        self.connection.execute(
            "UPDATE projects SET name=?1,updated_at=?2 WHERE id=?3",
            params![normalized_name(name)?, now(), id],
        )?;
        Ok(())
    }

    pub fn archive_project(&self, id: &str) -> Result<()> {
        self.project_by_id(id)?;
        self.connection.execute(
            "UPDATE projects SET archived_at=?1,updated_at=?1 WHERE id=?2",
            params![now(), id],
        )?;
        Ok(())
    }

    pub fn delete_project(&mut self, id: &str) -> Result<()> {
        self.project_by_id(id)?;
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "UPDATE conversations SET project_id=NULL,updated_at=?1 WHERE project_id=?2",
            params![now(), id],
        )?;
        transaction.execute("DELETE FROM projects WHERE id=?1", [id])?;
        transaction.commit()?;
        Ok(())
    }

    pub fn list_conversations(&self, project_id: Option<&str>) -> Result<Vec<ConversationSummary>> {
        let mut sql = String::from(
            "SELECT id,project_id,title,model,updated_at FROM conversations c
             WHERE archived_at IS NULL AND (project_id IS NULL OR EXISTS
             (SELECT 1 FROM projects p WHERE p.id=c.project_id AND p.archived_at IS NULL))",
        );
        if project_id.is_some() {
            sql.push_str(" AND project_id=?1");
        }
        sql.push_str(" ORDER BY updated_at DESC,rowid DESC");
        let mut statement = self.connection.prepare(&sql)?;
        let rows = if let Some(id) = project_id {
            statement.query_map([id], conversation_from_row)?
        } else {
            statement.query_map([], conversation_from_row)?
        };
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("读取会话失败")
    }

    pub fn create_conversation(
        &self,
        project_id: Option<&str>,
        title: &str,
        model: &str,
    ) -> Result<ConversationSummary> {
        self.ensure_active_project(project_id)?;
        let id = new_id();
        let title = if title.trim().is_empty() {
            "新会话"
        } else {
            title.trim()
        };
        self.connection.execute(
            "INSERT INTO conversations(id,project_id,title,model,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?5)",
            params![id,project_id,title,model,now()],
        )?;
        self.conversation_by_id(&id)
    }

    pub fn rename_conversation(&self, id: &str, title: &str) -> Result<()> {
        let title = normalized_name(title)?;
        let changed = self
            .connection
            .execute(
                "UPDATE conversations SET title = ?1, updated_at = ?2
                 WHERE id = ?3 AND archived_at IS NULL",
                params![title, now(), id],
            )
            .context("重命名会话失败")?;
        if changed == 0 {
            bail!("会话不存在或已归档");
        }
        Ok(())
    }

    pub fn move_conversation(&self, id: &str, project_id: Option<&str>) -> Result<()> {
        self.ensure_active_project(project_id)?;
        let changed = self.connection.execute(
            "UPDATE conversations SET project_id = ?1, updated_at = ?2
                 WHERE id = ?3 AND archived_at IS NULL",
            params![project_id, now(), id],
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
        self.conversation_by_id(id)?;
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
        let escaped = keyword
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        let pattern = format!("%{escaped}%");
        let mut statement = self.connection.prepare(
            "SELECT DISTINCT c.id, c.project_id, c.title, c.model, c.updated_at
             FROM conversations c
             LEFT JOIN projects g ON g.id = c.project_id
             LEFT JOIN messages m ON m.conversation_id = c.id
             WHERE c.archived_at IS NULL AND (c.project_id IS NULL OR g.archived_at IS NULL)
               AND (c.title LIKE ?1 ESCAPE '\\' OR m.content LIKE ?1 ESCAPE '\\')
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
        self.ensure_active_project(
            self.conversation_by_id(conversation_id)?
                .project_id
                .as_deref(),
        )?;
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
            "UPDATE conversations SET updated_at = ?1, model = ?2,
             title = CASE WHEN title = '新会话' THEN ?3 ELSE title END WHERE id = ?4",
            params![
                timestamp,
                model,
                user_content.chars().take(24).collect::<String>(),
                conversation_id
            ],
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

    pub fn delete_conversation(&mut self, id: &str) -> Result<()> {
        self.conversation_by_id(id)?;
        let transaction = self.connection.transaction()?;
        transaction.execute("DELETE FROM runs WHERE conversation_id = ?1", [id])?;
        transaction.execute("DELETE FROM messages WHERE conversation_id = ?1", [id])?;
        transaction.execute("DELETE FROM conversations WHERE id = ?1", [id])?;
        transaction.commit()?;
        Ok(())
    }

    pub fn message_count(&self, id: &str) -> Result<usize> {
        Ok(self.connection.query_row(
            "SELECT COUNT(*) FROM messages WHERE conversation_id = ?1
             AND (role != 'assistant' OR content != '')",
            [id],
            |row| row.get::<_, i64>(0),
        )? as usize)
    }

    pub fn save_partial(&self, run: &RunHandle, content: &str) -> Result<()> {
        self.connection.execute(
            "UPDATE messages SET content = ?1 WHERE id = ?2 AND status = 'streaming'",
            params![content, run.assistant_id],
        )?;
        Ok(())
    }

    pub fn export_conversation(&self, id: &str) -> Result<String> {
        let conversation = self.conversation_by_id(id)?;
        let mut output = format!("# {}\n\n", conversation.title);
        for message in self.load_messages(id, false)? {
            output.push_str(&format!(
                "## {} #{} ({})\n\n{}\n\n",
                message.role, message.sequence, message.status, message.content
            ));
            if let Some(error) = message.error {
                output.push_str(&format!("错误：{error}\n\n"));
            }
        }
        Ok(output)
    }

    pub fn conversation_by_id(&self, id: &str) -> Result<ConversationSummary> {
        self.connection.query_row(
            "SELECT id,project_id,title,model,updated_at FROM conversations c WHERE id=?1 AND archived_at IS NULL
             AND (project_id IS NULL OR EXISTS(SELECT 1 FROM projects p WHERE p.id=c.project_id AND p.archived_at IS NULL))",
            [id], conversation_from_row,
        ).optional()?.context("会话不存在或已归档")
    }

    pub fn resolve_conversation(&self, value: &str) -> Result<ConversationSummary> {
        let conversations = self.list_conversations(None)?;
        conversations
            .iter()
            .find(|item| item.id == value)
            .cloned()
            .or_else(|| {
                conversations
                    .into_iter()
                    .find(|item| item.title.eq_ignore_ascii_case(value.trim()))
            })
            .context("会话不存在或已归档")
    }

    fn ensure_active_project(&self, id: Option<&str>) -> Result<()> {
        if let Some(id) = id {
            self.project_by_id(id)?;
        }
        Ok(())
    }
}

fn conversation_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ConversationSummary> {
    Ok(ConversationSummary {
        id: row.get(0)?,
        project_id: row.get(1)?,
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
    use super::*;

    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(new_id());
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            fs::remove_dir(&self.0).unwrap();
        }
    }

    #[test]
    fn projects_bind_existing_directories_and_reopening_is_idempotent() {
        let directory = Directory::new();
        let store = Store::open(":memory:").unwrap();
        let first = store
            .create_project(directory.0.to_str().unwrap(), "")
            .unwrap();
        let again = store
            .create_project(directory.0.join(".").to_str().unwrap(), "other")
            .unwrap();
        assert_eq!(first.id, again.id);
        assert_eq!(store.list_projects(false).unwrap().len(), 1);
        assert!(Path::new(&first.directory).is_absolute());
        assert!(store.create_project("relative/path", "bad").is_err());
        assert!(
            store
                .create_project(directory.0.join("missing").to_str().unwrap(), "bad")
                .is_err()
        );
        store.archive_project(&first.id).unwrap();
        assert!(store.list_projects(false).unwrap().is_empty());
        assert_eq!(
            store
                .create_project(directory.0.to_str().unwrap(), "")
                .unwrap()
                .id,
            first.id
        );
    }

    #[test]
    fn removing_project_retains_conversations_and_does_not_delete_the_directory() {
        let directory = Directory::new();
        let mut store = Store::open(":memory:").unwrap();
        let project = store
            .create_project(directory.0.to_str().unwrap(), "fixture")
            .unwrap();
        let conversation = store
            .create_conversation(Some(&project.id), "history", "model")
            .unwrap();
        let run = store
            .start_run(&conversation.id, "question", "model")
            .unwrap();
        store.complete_run(&run, "answer").unwrap();
        store.delete_project(&project.id).unwrap();
        assert!(directory.0.is_dir());
        assert_eq!(
            store
                .conversation_by_id(&conversation.id)
                .unwrap()
                .project_id,
            None
        );
        assert_eq!(
            store.load_messages(&conversation.id, true).unwrap().len(),
            2
        );
    }

    #[test]
    fn conversations_can_live_outside_projects_and_move_without_losing_history() {
        let directory = Directory::new();
        let mut store = Store::open(":memory:").unwrap();
        let project = store
            .create_project(directory.0.to_str().unwrap(), "fixture")
            .unwrap();
        let conversation = store.create_conversation(None, "local", "model").unwrap();
        let run = store
            .start_run(&conversation.id, "question", "model")
            .unwrap();
        store
            .interrupt_run(&run, "partial", "network interrupted")
            .unwrap();
        store
            .move_conversation(&conversation.id, Some(&project.id))
            .unwrap();
        assert_eq!(
            store.list_conversations(Some(&project.id)).unwrap().len(),
            1
        );
        store.archive_project(&project.id).unwrap();
        assert!(store.list_conversations(None).unwrap().is_empty());
        assert!(
            store
                .start_run(&conversation.id, "cannot write", "model")
                .is_err()
        );
        store
            .create_project(directory.0.to_str().unwrap(), "")
            .unwrap();
        assert_eq!(
            store.load_messages(&conversation.id, false).unwrap()[1].content,
            "partial"
        );
        store.move_conversation(&conversation.id, None).unwrap();
        assert!(
            store
                .list_conversations(Some(&project.id))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn search_treats_wildcards_as_literal_and_excludes_archived_projects() {
        let directory = Directory::new();
        let mut store = Store::open(":memory:").unwrap();
        let project = store
            .create_project(directory.0.to_str().unwrap(), "fixture")
            .unwrap();
        let exact = store
            .create_conversation(Some(&project.id), "100%_完成", "model")
            .unwrap();
        store
            .create_conversation(None, "100 percent", "model")
            .unwrap();
        let run = store
            .start_run(&exact.id, "文件路径 C:\\repo_a", "model")
            .unwrap();
        store.complete_run(&run, "answer").unwrap();
        for query in ["%", "_", "C:\\repo_a"] {
            let found = store.search(query).unwrap();
            assert_eq!(found.len(), 1);
            assert_eq!(found[0].id, exact.id);
        }
        store.archive_project(&project.id).unwrap();
        assert!(store.search("完成").unwrap().is_empty());
    }
}
