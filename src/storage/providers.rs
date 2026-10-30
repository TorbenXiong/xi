//! Provider 与模型能力存储；凭据不会通过列表接口返回。
use super::Store;
use crate::model::{Config, validate_models};
use crate::protocol::{ModelDescriptor, ProviderSummary, ReasoningSource};
use crate::util::{new_id, unix_timestamp};
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};

impl Store {
    pub(super) fn init_provider_tables(&self) -> Result<()> {
        self.connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS providers (
                id TEXT PRIMARY KEY,name TEXT NOT NULL UNIQUE,base_url TEXT NOT NULL,api_key TEXT NOT NULL,
                created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS provider_models (
                provider_id TEXT NOT NULL,model TEXT NOT NULL,reasoning_efforts TEXT NOT NULL,reasoning_source TEXT NOT NULL,
                PRIMARY KEY(provider_id,model),
                FOREIGN KEY(provider_id) REFERENCES providers(id) ON DELETE CASCADE
             );"
        )?;
        Ok(())
    }

    pub fn providers(&self) -> Result<Vec<ProviderSummary>> {
        let active = active_provider_id(&self.connection)?;
        let mut statement = self.connection.prepare(
            "SELECT id,name,base_url,length(trim(api_key))>0 FROM providers ORDER BY created_at,id",
        )?;
        let mut providers = statement
            .query_map([], |row| {
                Ok(ProviderSummary {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    base_url: row.get(2)?,
                    api_key_configured: row.get(3)?,
                    active: false,
                    models: Vec::new(),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for provider in &mut providers {
            provider.active = active.as_deref() == Some(&provider.id);
            provider.models = load_models(&self.connection, &provider.id)?;
        }
        Ok(providers)
    }

    pub fn provider(&self, id: &str) -> Result<Config> {
        self.connection
            .query_row(
                "SELECT base_url,api_key FROM providers WHERE id=?1",
                [id],
                |row| {
                    Ok(Config {
                        base_url: row.get(0)?,
                        api_key: row.get(1)?,
                        model: String::new(),
                    })
                },
            )
            .optional()?
            .context("模型配置不存在")
    }

    pub fn model_descriptor(&self, provider_id: &str, model: &str) -> Result<ModelDescriptor> {
        load_models(&self.connection, provider_id)?
            .into_iter()
            .find(|item| item.id == model)
            .context("所选模型不存在，请先获取并保存模型列表")
    }

    pub fn create_provider(
        &mut self,
        name: &str,
        config: &Config,
        models: &[ModelDescriptor],
    ) -> Result<String> {
        validate_models(models)?;
        let id = new_id("provider");
        let transaction = self.connection.transaction()?;
        let select = active_provider_id(&transaction)?.is_none();
        transaction.execute(
            "INSERT INTO providers(id,name,base_url,api_key,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?5)",
            params![id,name,config.base_url,config.api_key,unix_timestamp()],
        ).context("无法保存模型配置，名称可能已存在")?;
        replace_models(&transaction, &id, models, false)?;
        if select {
            transaction.execute(
                "INSERT OR REPLACE INTO app_state(key,value) VALUES ('active_provider_id',?1)",
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
        models: &[ModelDescriptor],
    ) -> Result<()> {
        validate_models(models)?;
        let transaction = self.connection.transaction()?;
        let changed = transaction
            .execute(
                "UPDATE providers SET name=?1,base_url=?2,api_key=?3,updated_at=?4 WHERE id=?5",
                params![name, config.base_url, config.api_key, unix_timestamp(), id],
            )
            .context("无法更新模型配置，名称可能已存在")?;
        if changed == 0 {
            bail!("模型配置不存在");
        }
        replace_models(&transaction, id, models, false)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn cache_models(
        &mut self,
        id: &str,
        models: &[ModelDescriptor],
    ) -> Result<Vec<ModelDescriptor>> {
        validate_models(models)?;
        let transaction = self.connection.transaction()?;
        replace_models(&transaction, id, models, true)?;
        let result = load_models(&transaction, id)?;
        transaction.commit()?;
        Ok(result)
    }

    pub fn select_provider(&self, id: &str) -> Result<()> {
        self.provider(id)?;
        self.set_state("active_provider_id", id)
    }

    pub fn delete_provider(&mut self, id: &str) -> Result<()> {
        let transaction = self.connection.transaction()?;
        let count: i64 =
            transaction.query_row("SELECT COUNT(*) FROM providers", [], |row| row.get(0))?;
        if count <= 1 {
            bail!("至少保留一个模型配置");
        }
        let active = active_provider_id(&transaction)?;
        if transaction.execute("DELETE FROM providers WHERE id=?1", [id])? == 0 {
            bail!("模型配置不存在");
        }
        if active.as_deref() == Some(id) {
            let next = active_provider_id(&transaction)?.context("没有可用模型配置")?;
            transaction.execute(
                "INSERT OR REPLACE INTO app_state(key,value) VALUES ('active_provider_id',?1)",
                [next],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }
}

fn active_provider_id(connection: &Connection) -> Result<Option<String>> {
    let stored:Option<String>=connection.query_row(
        "SELECT p.id FROM providers p JOIN app_state s ON s.value=p.id WHERE s.key='active_provider_id'",[],|row| row.get(0),
    ).optional()?;
    if stored.is_some() {
        return Ok(stored);
    }
    Ok(connection
        .query_row(
            "SELECT id FROM providers ORDER BY created_at,id LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?)
}

fn load_models(connection: &Connection, id: &str) -> Result<Vec<ModelDescriptor>> {
    let mut statement=connection.prepare("SELECT model,reasoning_efforts,reasoning_source FROM provider_models WHERE provider_id=?1 ORDER BY model")?;
    let rows = statement.query_map([id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    rows.map(|row| {
        let (id, efforts, source) = row?;
        Ok(ModelDescriptor {
            id,
            reasoning_efforts: serde_json::from_str(&efforts)?,
            reasoning_source: serde_json::from_str(&source)?,
        })
    })
    .collect()
}

fn replace_models(
    connection: &Connection,
    id: &str,
    models: &[ModelDescriptor],
    preserve_custom: bool,
) -> Result<()> {
    let previous = if preserve_custom {
        load_models(connection, id)?
    } else {
        Vec::new()
    };
    connection.execute("DELETE FROM provider_models WHERE provider_id=?1", [id])?;
    for model in models {
        let model = previous
            .iter()
            .find(|saved| saved.id == model.id && saved.reasoning_source == ReasoningSource::Custom)
            .unwrap_or(model);
        connection.execute(
            "INSERT INTO provider_models(provider_id,model,reasoning_efforts,reasoning_source) VALUES (?1,?2,?3,?4)",
            params![id,model.id,serde_json::to_string(&model.reasoning_efforts)?,serde_json::to_string(&model.reasoning_source)?],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn model_refresh_replaces_stale_entries_but_preserves_custom_capabilities() {
        let mut store = Store::open(":memory:").unwrap();
        let config = Config {
            base_url: "http://localhost/v1".into(),
            api_key: "FIXTURE_ONLY".into(),
            model: String::new(),
        };
        let custom = ModelDescriptor {
            id: "custom-model".into(),
            reasoning_efforts: vec!["max".into()],
            reasoning_source: ReasoningSource::Custom,
        };
        let old = ModelDescriptor {
            id: "removed-model".into(),
            reasoning_efforts: vec![],
            reasoning_source: ReasoningSource::Unknown,
        };
        let id = store
            .create_provider("fixture", &config, &[custom.clone(), old])
            .unwrap();
        let fetched = ModelDescriptor {
            id: "custom-model".into(),
            reasoning_efforts: vec![],
            reasoning_source: ReasoningSource::Unknown,
        };
        assert_eq!(store.cache_models(&id, &[fetched]).unwrap(), vec![custom]);
        store.update_provider(&id, "fixture", &config, &[]).unwrap();
        assert!(store.providers().unwrap()[0].models.is_empty());
        assert_eq!(store.provider(&id).unwrap().api_key, "FIXTURE_ONLY");
    }
}
