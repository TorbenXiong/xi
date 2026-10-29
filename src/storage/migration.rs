//! 将客户端原型的工作区快照导入主线关系表；来源只读，事务成功后标记一次。
use super::{Store, new_id, now};
use crate::protocol::{Group, Session};
use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags, OptionalExtension, Transaction, params};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Deserialize, Default)]
struct Snapshot {
    groups: Vec<Group>,
    sessions: Vec<Session>,
    active_session_id: Option<String>,
}
struct LegacyProvider {
    id: String,
    name: String,
    url: String,
    key: String,
    model: String,
    models: Vec<String>,
}

impl Store {
    pub(super) fn migrate_workspace_snapshot(&mut self) -> Result<()> {
        if self.get_state("client_snapshot_imported")?.is_some() {
            return Ok(());
        }
        let snapshot = self
            .get_state("workspace")?
            .map(|text| serde_json::from_str::<Snapshot>(&text))
            .transpose()
            .context("旧工作区数据格式错误")?;
        let default = self.ungrouped_id()?;
        let transaction = self.connection.transaction()?;
        if let Some(snapshot) = snapshot {
            import_snapshot(&transaction, snapshot, &default)?;
        }
        transaction.execute(
            "INSERT OR REPLACE INTO app_state VALUES ('client_snapshot_imported', '1')",
            [],
        )?;
        transaction.pragma_update(None, "user_version", 2)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn import_client_database(&mut self, path: &Path) -> Result<()> {
        if !path.is_file() {
            return Ok(());
        }
        let path = path.canonicalize().context("无法确定旧数据库路径")?;
        let marker = format!("imported_client:{}", path.display());
        if self.get_state(&marker)?.is_some() {
            return Ok(());
        }
        let mut source_connection =
            Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
                .context("无法读取旧客户端数据库")?;
        let source = source_connection.transaction()?;
        let state_exists: bool = source.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'app_state')", [], |r| r.get(0))?;
        if !state_exists {
            return Ok(());
        }
        let snapshot = source
            .query_row(
                "SELECT value FROM app_state WHERE key = 'workspace'",
                [],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .map(|text| serde_json::from_str::<Snapshot>(&text))
            .transpose()
            .context("旧工作区数据格式错误")?;
        let provider_exists: bool = source.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'providers')", [], |r| r.get(0))?;
        let mut providers = Vec::new();
        if provider_exists {
            let mut statement = source.prepare(
                "SELECT id,name,base_url,api_key,model FROM providers ORDER BY created_at,id",
            )?;
            for row in statement.query_map([], |row| {
                Ok(LegacyProvider {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    url: row.get(2)?,
                    key: row.get(3)?,
                    model: row.get(4)?,
                    models: Vec::new(),
                })
            })? {
                let mut provider = row?;
                let has_models: bool = source.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='provider_models')", [], |r| r.get(0))?;
                if has_models {
                    let mut models =
                        source.prepare("SELECT model FROM provider_models WHERE provider_id=?1")?;
                    provider.models = models
                        .query_map([&provider.id], |r| r.get(0))?
                        .collect::<rusqlite::Result<_>>()?;
                }
                providers.push(provider);
            }
        }
        let active_provider = source
            .query_row(
                "SELECT value FROM app_state WHERE key='active_provider_id'",
                [],
                |r| r.get::<_, String>(0),
            )
            .optional()?;
        source.commit()?;
        let default = self.ungrouped_id()?;
        let transaction = self.connection.transaction()?;
        if let Some(snapshot) = snapshot {
            import_snapshot(&transaction, snapshot, &default)?;
        }
        for mut provider in providers {
            let exists: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM providers WHERE id=?1)",
                [&provider.id],
                |r| r.get(0),
            )?;
            if exists {
                continue;
            }
            let original_name = provider.name.clone();
            let mut suffix = 1;
            while transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM providers WHERE name=?1)",
                [&provider.name],
                |r| r.get::<_, bool>(0),
            )? {
                provider.name = format!("{original_name} (导入 {suffix})");
                suffix += 1;
            }
            transaction.execute("INSERT INTO providers (id,name,base_url,api_key,model,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?6)",
                params![provider.id,provider.name,provider.url,provider.key,provider.model,now()])?;
            for model in provider.models {
                transaction.execute(
                    "INSERT OR IGNORE INTO provider_models VALUES (?1,?2)",
                    params![provider.id, model],
                )?;
            }
        }
        if let Some(id) = active_provider {
            transaction.execute("INSERT OR IGNORE INTO app_state (key,value) SELECT 'active_provider_id',id FROM providers WHERE id=?1", [id])?;
        }
        transaction.execute(
            "INSERT INTO app_state (key,value) VALUES (?1,'1')",
            [marker],
        )?;
        transaction.commit().context("无法提交旧客户端数据迁移")?;
        Ok(())
    }
}

fn import_snapshot(transaction: &Transaction<'_>, snapshot: Snapshot, default: &str) -> Result<()> {
    let mut groups = HashMap::new();
    for group in snapshot.groups {
        if group.is_default {
            groups.insert(group.id, default.to_owned());
            continue;
        }
        let existing = transaction.query_row("SELECT id FROM conversation_groups WHERE (id=?1 OR name=?2 COLLATE NOCASE) AND archived_at IS NULL LIMIT 1",
            params![group.id,group.name], |row| row.get::<_,String>(0)).optional()?;
        let id = existing.unwrap_or_else(|| group.id.clone());
        transaction.execute("INSERT OR IGNORE INTO conversation_groups (id,name,created_at,updated_at) VALUES (?1,?2,?3,?3)", params![id,group.name,now()])?;
        groups.insert(group.id, id);
    }
    for session in snapshot.sessions {
        let exists: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM conversations WHERE id=?1)",
            [&session.id],
            |r| r.get(0),
        )?;
        if exists {
            continue;
        }
        let group = groups
            .get(&session.group_id)
            .map(String::as_str)
            .unwrap_or(default);
        transaction.execute("INSERT INTO conversations (id,group_id,title,model,created_at,updated_at) VALUES (?1,?2,?3,'',?4,?4)",
            params![session.id,group,session.title,now()])?;
        for (sequence, message) in session.messages.into_iter().enumerate() {
            transaction.execute("INSERT INTO messages (id,conversation_id,sequence,role,content,status,created_at,completed_at) VALUES (?1,?2,?3,?4,?5,'complete',?6,?6)",
                params![new_id(),session.id,sequence as i64,message.role,message.content,now()])?;
        }
    }
    if let Some(id) = snapshot.active_session_id {
        transaction.execute("INSERT OR IGNORE INTO app_state (key,value) SELECT 'active_conversation_id',id FROM conversations WHERE id=?1", [id])?;
        transaction.execute("INSERT OR IGNORE INTO app_state (key,value) SELECT 'active_group_id',group_id FROM conversations WHERE id=(SELECT value FROM app_state WHERE key='active_conversation_id')", [])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_read_only_source_and_rolls_back_invalid_messages() {
        let directory = std::env::temp_dir().join(new_id());
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("legacy.sqlite3");
        let source = Connection::open(&path).unwrap();
        source.execute_batch("CREATE TABLE app_state(key TEXT PRIMARY KEY,value TEXT NOT NULL);
            CREATE TABLE providers(id TEXT PRIMARY KEY,name TEXT,base_url TEXT,api_key TEXT,model TEXT,created_at INTEGER);
            CREATE TABLE provider_models(provider_id TEXT,model TEXT);
            INSERT INTO providers VALUES ('legacy-provider','fixture','http://localhost/v1','FIXTURE_ONLY','',1);
            INSERT INTO provider_models VALUES ('legacy-provider','fixture-model');
            INSERT INTO app_state VALUES ('active_provider_id','legacy-provider');").unwrap();
        let snapshot = serde_json::json!({"groups":[{"id":"old-default","name":"未分组","is_default":true}],
            "sessions":[{"id":"legacy-session","group_id":"old-default","title":"fixture","messages":[{"role":"invalid","content":"fixture"}]}],"active_session_id":"legacy-session"});
        source
            .execute(
                "INSERT INTO app_state VALUES ('workspace',?1)",
                [snapshot.to_string()],
            )
            .unwrap();
        drop(source);
        let mut target = Store::open(":memory:").unwrap();
        assert!(target.import_client_database(&path).is_err());
        assert!(target.list_conversations(None).unwrap().is_empty());
        assert!(target.providers().unwrap().is_empty());
        let source = Connection::open(&path).unwrap();
        let mut snapshot = snapshot;
        snapshot["sessions"][0]["messages"][0]["role"] = "user".into();
        source
            .execute(
                "UPDATE app_state SET value=?1 WHERE key='workspace'",
                [snapshot.to_string()],
            )
            .unwrap();
        drop(source);
        let original = std::fs::read(&path).unwrap();
        target.import_client_database(&path).unwrap();
        target.import_client_database(&path).unwrap();
        assert_eq!(target.list_conversations(None).unwrap().len(), 1);
        assert_eq!(target.providers().unwrap()[0].models, ["fixture-model"]);
        assert_eq!(
            target.provider(Some("legacy-provider")).unwrap().api_key,
            "FIXTURE_ONLY"
        );
        assert_eq!(original, std::fs::read(&path).unwrap());
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
    #[test]
    fn snapshot_migration_is_atomic_idempotent_and_preserves_main_history() {
        let mut store = Store::open(":memory:").unwrap();
        let group = store.create_group("main").unwrap();
        let main = store
            .create_conversation(&group.id, "main history", "model")
            .unwrap();
        store
            .connection
            .execute(
                "DELETE FROM app_state WHERE key='client_snapshot_imported'",
                [],
            )
            .unwrap();
        store.set_state("workspace", r#"{"groups":[{"id":"old-default","name":"未分组","is_default":true}],"sessions":[{"id":"old-session","group_id":"old-default","title":"client","messages":[{"role":"user","content":"fixture"}]}],"active_session_id":"old-session"}"#).unwrap();
        store.migrate_workspace_snapshot().unwrap();
        store.migrate_workspace_snapshot().unwrap();
        assert_eq!(store.list_conversations(None).unwrap().len(), 2);
        assert!(store.conversation_by_id(&main.id).is_ok());
        assert_eq!(
            store.conversation_by_id("old-session").unwrap().group_id,
            store.ungrouped_id().unwrap()
        );
        assert_eq!(
            store.load_messages("old-session", true).unwrap()[0].content,
            "fixture"
        );
    }
}
