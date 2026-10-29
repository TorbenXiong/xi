//! Provider configuration stored alongside relational conversations.
use super::Store;
use crate::model::Config;
use crate::protocol::ProviderSummary;
use crate::util::{new_id, unix_timestamp};
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};

impl Store {
    pub(super) fn init_provider_tables(&self) -> Result<()> {
        self.connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS app_state (key TEXT PRIMARY KEY, value TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS providers (
             id TEXT PRIMARY KEY,
             name TEXT NOT NULL UNIQUE,
             base_url TEXT NOT NULL,
             api_key TEXT NOT NULL,
             model TEXT NOT NULL,
             created_at INTEGER NOT NULL,
             updated_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS provider_models (
             provider_id TEXT NOT NULL,
             model TEXT NOT NULL,
             PRIMARY KEY (provider_id, model),
             FOREIGN KEY (provider_id) REFERENCES providers(id) ON DELETE CASCADE
         );",
        )?;
        Ok(())
    }
    pub fn providers(&self) -> Result<Vec<ProviderSummary>> {
        let connection = &self.connection;
        let active = active_provider_id(connection)?;
        let mut statement = connection.prepare(
            "SELECT id, name, base_url, model, length(trim(api_key)) > 0 FROM providers ORDER BY created_at"
        )?;
        let rows = statement.query_map([], |row| {
            Ok(ProviderSummary {
                id: row.get(0)?,
                name: row.get(1)?,
                base_url: row.get(2)?,
                model: row.get(3)?,
                api_key_configured: row.get(4)?,
                active: false,
                models: Vec::new(),
            })
        })?;
        let mut providers = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        for provider in &mut providers {
            provider.active = active.as_deref() == Some(&provider.id);
            let mut statement = connection.prepare(
                "SELECT model FROM provider_models WHERE provider_id = ?1 ORDER BY model",
            )?;
            provider.models = statement
                .query_map([&provider.id], |row| row.get(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
        }
        Ok(providers)
    }

    pub fn provider(&self, id: Option<&str>) -> Result<Config> {
        let connection = &self.connection;
        let id = match id {
            Some(id) => id.to_owned(),
            None => active_provider_id(connection)?.context("请先配置模型服务")?,
        };
        connection
            .query_row(
                "SELECT base_url, api_key, model FROM providers WHERE id = ?1",
                [&id],
                |row| {
                    Ok(Config {
                        base_url: row.get(0)?,
                        api_key: row.get(1)?,
                        model: row.get(2)?,
                    })
                },
            )
            .optional()?
            .context("模型配置不存在")
    }

    pub fn create_provider(
        &mut self,
        name: &str,
        config: &Config,
        models: &[String],
    ) -> Result<String> {
        let id = new_id("provider");
        let now = unix_timestamp();
        let connection = &mut self.connection;
        let transaction = connection.transaction()?;
        let select = active_provider_id(&transaction)?.is_none();
        transaction.execute(
            "INSERT INTO providers (id, name, base_url, api_key, model, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![id, name, config.base_url, config.api_key, config.model, now],
        ).context("无法保存模型配置")?;
        cache_models(&transaction, &id, models)?;
        if select {
            transaction.execute(
                "INSERT OR REPLACE INTO app_state (key, value) VALUES ('active_provider_id', ?1)",
                [&id],
            )?;
        }
        transaction.commit()?;
        Ok(id)
    }

    pub fn update_provider(
        &mut self,
        id: &str,
        name: &str,
        config: &Config,
        models: &[String],
    ) -> Result<()> {
        let connection = &mut self.connection;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE providers SET name = ?1, base_url = ?2, api_key = ?3, model = ?4, updated_at = ?5 WHERE id = ?6",
            params![name, config.base_url, config.api_key, config.model, unix_timestamp(), id],
        ).context("无法更新模型配置")?;
        if changed == 0 {
            bail!("模型配置不存在");
        }
        cache_models(&transaction, id, models)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn cache_models(&mut self, id: &str, models: &[String]) -> Result<()> {
        let connection = &mut self.connection;
        let transaction = connection.transaction()?;
        cache_models(&transaction, id, models)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn select_provider(&self, id: &str) -> Result<()> {
        let connection = &self.connection;
        let exists: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM providers WHERE id = ?1)",
            [id],
            |r| r.get(0),
        )?;
        if !exists {
            bail!("模型配置不存在");
        }
        connection.execute(
            "INSERT OR REPLACE INTO app_state (key, value) VALUES ('active_provider_id', ?1)",
            [id],
        )?;
        Ok(())
    }

    pub fn delete_provider(&mut self, id: &str) -> Result<()> {
        let connection = &mut self.connection;
        let transaction = connection.transaction()?;
        let count: i64 =
            transaction.query_row("SELECT COUNT(*) FROM providers", [], |row| row.get(0))?;
        if count <= 1 {
            bail!("至少保留一个模型配置");
        }
        let active = active_provider_id(&transaction)?;
        let changed = transaction.execute("DELETE FROM providers WHERE id = ?1", [id])?;
        if changed == 0 {
            bail!("模型配置不存在");
        }
        if active.as_deref() == Some(id) {
            let next = active_provider_id(&transaction)?.context("没有可用模型配置")?;
            transaction.execute(
                "INSERT OR REPLACE INTO app_state (key, value) VALUES ('active_provider_id', ?1)",
                [&next],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }
}

fn active_provider_id(connection: &Connection) -> Result<Option<String>> {
    let stored: Option<String> = connection.query_row(
        "SELECT p.id FROM providers p JOIN app_state s ON s.value = p.id WHERE s.key = 'active_provider_id'",
        [], |row| row.get(0),
    ).optional()?;
    if stored.is_some() {
        return Ok(stored);
    }
    Ok(connection
        .query_row(
            "SELECT id FROM providers ORDER BY created_at LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?)
}

fn cache_models(connection: &Connection, id: &str, models: &[String]) -> Result<()> {
    for model in models {
        connection.execute(
            "INSERT OR IGNORE INTO provider_models (provider_id, model) VALUES (?1, ?2)",
            params![id, model],
        )?;
    }
    Ok(())
}
