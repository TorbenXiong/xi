//! UI、HTTP 与 CLI 共用业务入口；数据库锁不跨越模型网络调用。
use std::fs::{File, OpenOptions};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::model::{Config, ModelRuntime};
use crate::protocol::*;
use crate::storage::{ConversationSummary, ProjectSummary, Store, StoredMessage};
use anyhow::{Context, Result, bail};

pub(crate) struct AppServer {
    store: Mutex<Store>,
    model: ModelRuntime,
    chat_lock: Mutex<()>,
    _instance_lock: File,
}

impl AppServer {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let instance_lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path.with_extension("lock"))?;
        instance_lock
            .try_lock()
            .context("数据库已被另一 Runtime 使用，请连接已有服务，或退出后重试")?;
        let mut store = Store::open(path)?;
        store.mark_stale_runs_interrupted()?;
        Ok(Self {
            store: Mutex::new(store),
            model: ModelRuntime::new()?,
            chat_lock: Mutex::new(()),
            _instance_lock: instance_lock,
        })
    }

    fn store(&self) -> Result<MutexGuard<'_, Store>> {
        self.store
            .lock()
            .map_err(|_| anyhow::anyhow!("数据库状态不可用"))
    }

    fn chat_guard(&self) -> Result<MutexGuard<'_, ()>> {
        self.chat_lock
            .lock()
            .map_err(|_| anyhow::anyhow!("会话状态不可用"))
    }

    pub fn providers(&self) -> Result<Vec<ProviderSummary>> {
        self.store()?.providers()
    }

    pub fn provider_models(&self, input: ProviderModelsInput) -> Result<Vec<ModelDescriptor>> {
        let stored = input
            .id
            .as_deref()
            .map(|id| self.store()?.provider(id))
            .transpose()?;
        let config = Config {
            base_url: if input.base_url.trim().is_empty() {
                stored
                    .as_ref()
                    .map(|c| c.base_url.clone())
                    .unwrap_or_default()
            } else {
                input.base_url.trim().trim_end_matches('/').to_owned()
            },
            api_key: if input.api_key.trim().is_empty() {
                stored
                    .as_ref()
                    .map(|c| c.api_key.clone())
                    .unwrap_or_default()
            } else {
                input.api_key.trim().to_owned()
            },
            model: String::new(),
        };
        if config.base_url.is_empty() || config.api_key.is_empty() {
            bail!("请先填写 API 地址和 API Key");
        }
        config.validate_base_url()?;
        let mut models = self.model.models(&config)?;
        if let Some(id) = input.id {
            let mut store = self.store()?;
            let current = store.provider(&id)?;
            // 编辑中的凭据和并发更新不能污染已保存服务的模型缓存。
            if current.base_url == config.base_url && current.api_key == config.api_key {
                models = store.cache_models(&id, &models)?;
            }
        }
        Ok(models)
    }

    pub fn create_provider(&self, input: ProviderInput) -> Result<String> {
        let (name, config) = provider_config(&input)?;
        self.store()?.create_provider(&name, &config, &input.models)
    }

    pub fn update_provider(&self, mut input: ProviderInput) -> Result<()> {
        let id = input.id.clone().context("更新模型配置需要 id")?;
        if input.api_key.trim().is_empty() {
            input.api_key = self.store()?.provider(&id)?.api_key;
        }
        let (name, config) = provider_config(&input)?;
        self.store()?
            .update_provider(&id, &name, &config, &input.models)
    }

    pub fn select_provider(&self, id: &str) -> Result<()> {
        self.store()?.select_provider(id)
    }
    pub fn delete_provider(&self, id: &str) -> Result<()> {
        self.store()?.delete_provider(id)
    }

    pub fn projects(&self) -> Result<Vec<ProjectSummary>> {
        self.store()?.list_projects(false)
    }
    pub fn resolve_project(&self, value: &str) -> Result<ProjectSummary> {
        self.store()?.resolve_project(value)
    }
    pub fn default_project(&self) -> Result<Option<ProjectSummary>> {
        let store = self.store()?;
        Ok(store
            .get_state("active_project_id")?
            .and_then(|id| store.project_by_id(&id).ok()))
    }
    pub fn select_project(&self, id: Option<&str>) -> Result<()> {
        let store = self.store()?;
        if let Some(id) = id {
            store.project_by_id(id)?;
            store.set_state("active_project_id", id)?;
        } else {
            store.remove_state("active_project_id")?;
        }
        store.remove_state("active_conversation_id")
    }
    pub fn resolve_session(&self, value: &str) -> Result<ConversationSummary> {
        self.store()?.resolve_conversation(value)
    }
    pub fn sessions(&self, project: Option<&str>) -> Result<Vec<ConversationSummary>> {
        self.store()?.list_conversations(project)
    }
    pub fn search(&self, keyword: &str) -> Result<Vec<ConversationSummary>> {
        self.store()?.search(keyword)
    }

    pub fn workspace(&self) -> Result<Workspace> {
        let store = self.store()?;
        let projects = store
            .list_projects(false)?
            .into_iter()
            .map(project_dto)
            .collect();
        let sessions = store
            .list_conversations(None)?
            .into_iter()
            .map(|s| {
                Ok(SessionSummary {
                    message_count: store.message_count(&s.id)?,
                    id: s.id,
                    project_id: s.project_id,
                    title: s.title,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let active_session_id = store
            .get_state("active_conversation_id")?
            .filter(|id| sessions.iter().any(|s| &s.id == id));
        Ok(Workspace {
            projects,
            sessions,
            active_session_id,
        })
    }

    pub fn history(&self, id: &str) -> Result<Vec<StoredMessage>> {
        let store = self.store()?;
        store.conversation_by_id(id)?;
        store.load_messages(id, false)
    }

    pub fn create_project(&self, input: ProjectCreateInput) -> Result<Project> {
        Ok(project_dto(
            self.store()?
                .create_project(&input.directory, &input.name)?,
        ))
    }
    pub fn rename_project(&self, input: IdNamedInput) -> Result<Project> {
        let store = self.store()?;
        store.rename_project(&input.id, &input.name)?;
        Ok(project_dto(store.project_by_id(&input.id)?))
    }
    pub fn delete_project(&self, id: &str) -> Result<()> {
        let _chat = self.chat_guard()?;
        self.store()?.delete_project(id)
    }
    pub fn archive_project(&self, id: &str) -> Result<()> {
        let _chat = self.chat_guard()?;
        self.store()?.archive_project(id)
    }

    pub fn create_session(&self, input: SessionCreateInput) -> Result<CreatedSession> {
        let store = self.store()?;
        let session = store.create_conversation(input.project_id.as_deref(), &input.title, "")?;
        store.set_state("active_conversation_id", &session.id)?;
        if let Some(id) = &session.project_id {
            store.set_state("active_project_id", id)?;
        } else {
            store.remove_state("active_project_id")?;
        }
        Ok(CreatedSession {
            session_id: session.id,
        })
    }

    pub fn select_session(&self, id: &str) -> Result<()> {
        let store = self.store()?;
        let session = store.conversation_by_id(id)?;
        store.set_state("active_conversation_id", id)?;
        if let Some(id) = &session.project_id {
            store.set_state("active_project_id", id)
        } else {
            store.remove_state("active_project_id")
        }
    }

    pub fn rename_session(&self, input: IdNamedInput) -> Result<()> {
        self.store()?.rename_conversation(&input.id, &input.name)
    }

    pub fn move_session(&self, id: &str, project_id: Option<&str>) -> Result<()> {
        let _chat = self.chat_guard()?;
        self.store()?.move_conversation(id, project_id)
    }

    pub fn archive_session(&self, id: &str) -> Result<()> {
        let _chat = self.chat_guard()?;
        self.store()?.archive_conversation(id)
    }

    pub fn delete_session(&self, id: &str) -> Result<()> {
        let _chat = self.chat_guard()?;
        self.store()?.delete_conversation(id)
    }

    pub fn export_session(&self, id: &str) -> Result<String> {
        self.store()?.export_conversation(id)
    }

    pub fn reset(&self, target: SessionTarget) -> Result<()> {
        let _chat = self.chat_guard()?;
        let mut store = self.store()?;
        store.clear_conversation(&target.session_id)?;
        Ok(())
    }

    pub fn chat(&self, input: ChatInput) -> Result<ChatOutput> {
        self.execute_chat(input, false, |_| Ok(()))
    }

    pub fn chat_stream(
        &self,
        input: ChatInput,
        on_delta: impl FnMut(&str) -> Result<()>,
    ) -> Result<ChatOutput> {
        self.execute_chat(input, true, on_delta)
    }

    fn execute_chat(
        &self,
        input: ChatInput,
        stream: bool,
        mut on_delta: impl FnMut(&str) -> Result<()>,
    ) -> Result<ChatOutput> {
        let content = input.content.trim();
        if content.is_empty() {
            bail!("消息不能为空");
        }
        let _chat = self.chat_guard()?;
        let (config, id, run, history) = {
            let mut store = self.store()?;
            let mut config = store.provider(&input.provider_id)?;
            config.model = input.model.trim().to_owned();
            config.validate()?;
            let descriptor = store.model_descriptor(&input.provider_id, &config.model)?;
            if let Some(effort) = &input.reasoning_effort
                && !descriptor.reasoning_efforts.contains(effort)
            {
                bail!("所选模型不支持推理强度：{effort}");
            }
            let id = input.session_id.clone();
            store.conversation_by_id(&id)?;
            let run = store.start_run(&id, content, &config.model)?;
            let history = store
                .load_messages(&id, true)?
                .into_iter()
                .map(|m| Message {
                    role: m.role,
                    content: m.content,
                })
                .collect::<Vec<_>>();
            (config, id, run, history)
        };
        let mut partial = String::new();
        let mut persisted_bytes = 0;
        let mut last_persist = Instant::now();
        let answer =
            self.model
                .complete_with(&config, &history, input.reasoning_effort, stream, |delta| {
                    partial.push_str(delta);
                    // 首块立即落库，后续节流；正常结束或失败仍保存完整结果。
                    if persisted_bytes == 0
                        || last_persist.elapsed() >= Duration::from_millis(250)
                        || partial.len() - persisted_bytes >= 8192
                    {
                        self.store()?.save_partial(&run, &partial)?;
                        persisted_bytes = partial.len();
                        last_persist = Instant::now();
                    }
                    on_delta(delta)
                });
        match answer {
            Ok(answer) => {
                self.store()?.complete_run(&run, &answer)?;
                Ok(ChatOutput {
                    session_id: id,
                    message: Message {
                        role: "assistant".to_owned(),
                        content: answer,
                    },
                })
            }
            Err(error) => {
                let mut store = self.store()?;
                if error.partial_content.is_empty() {
                    store.fail_run(&run, &error.to_string())?;
                } else {
                    store.interrupt_run(&run, &error.partial_content, &error.to_string())?;
                }
                Err(error.into())
            }
        }
    }
}

fn project_dto(project: ProjectSummary) -> Project {
    Project {
        id: project.id,
        name: project.name,
        directory: project.directory,
    }
}

fn provider_config(input: &ProviderInput) -> Result<(String, Config)> {
    let config = Config {
        base_url: input.base_url.trim().trim_end_matches('/').to_owned(),
        api_key: input.api_key.trim().to_owned(),
        model: String::new(),
    };
    if config.base_url.is_empty() || config.api_key.is_empty() {
        bail!("API 地址和 API Key 不能为空");
    }
    config.validate_base_url()?;
    let name = if input.name.trim().is_empty() {
        provider_name_from_url(&config.base_url)
    } else {
        input.name.trim().to_owned()
    };
    Ok((name, config))
}

fn provider_name_from_url(base_url: &str) -> String {
    let host = reqwest::Url::parse(base_url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_default();
    let labels = host
        .split('.')
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if labels.len() >= 2 {
        labels[labels.len() - 2].to_owned()
    } else {
        labels.first().copied().unwrap_or("provider").to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_runtime_does_not_interrupt_live_runs() {
        let directory = std::env::temp_dir().join(crate::util::new_id("xi-owner-test"));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("xi.db");
        let app = AppServer::open(&path).unwrap();
        let id = {
            let mut store = app.store().unwrap();
            let id = store
                .create_conversation(None, "fixture", "model")
                .unwrap()
                .id;
            store.start_run(&id, "question", "model").unwrap();
            id
        };
        assert!(AppServer::open(&path).is_err());
        assert_eq!(app.history(&id).unwrap()[1].status, "streaming");
        drop(app);
        let reopened = AppServer::open(&path).unwrap();
        assert_eq!(reopened.history(&id).unwrap()[1].status, "interrupted");
        drop(reopened);
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_file(path.with_extension("lock")).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn provider_name_uses_second_level_domain() {
        assert_eq!(
            provider_name_from_url("https://api.nassaapi.xyz/v1"),
            "nassaapi"
        );
        assert_eq!(
            provider_name_from_url("http://localhost:43127/v1"),
            "localhost"
        );
    }
}
