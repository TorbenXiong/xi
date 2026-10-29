//! UI、HTTP 与 CLI 共用业务入口；数据库锁不跨越模型网络调用。
use std::fs::{File, OpenOptions};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::model::{Config, ModelRuntime};
use crate::protocol::*;
use crate::storage::{ConversationSummary, GroupSummary, Store, StoredMessage};
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

    pub fn import_client_database(&self, path: &Path) -> Result<()> {
        self.store()?.import_client_database(path)
    }

    pub fn providers(&self) -> Result<Vec<ProviderSummary>> {
        self.store()?.providers()
    }

    pub fn provider_models(&self, input: ProviderModelsInput) -> Result<Vec<String>> {
        let stored = input
            .id
            .as_deref()
            .map(|id| self.store()?.provider(Some(id)))
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
        let models = self.model.models(&config)?;
        if let Some(id) = input.id {
            self.store()?.cache_models(&id, &models)?;
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
            input.api_key = self.store()?.provider(Some(&id))?.api_key;
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

    pub fn groups(&self) -> Result<Vec<GroupSummary>> {
        self.store()?.list_groups(false)
    }
    pub fn resolve_group(&self, value: &str) -> Result<GroupSummary> {
        self.store()?.resolve_group(value)
    }
    pub fn default_group(&self) -> Result<GroupSummary> {
        let store = self.store()?;
        if let Some(group) = store
            .get_state("active_group_id")?
            .and_then(|id| store.resolve_group(&id).ok())
        {
            return Ok(group);
        }
        store.group_by_id(&store.ungrouped_id()?)
    }
    pub fn select_group(&self, id: &str) -> Result<()> {
        let store = self.store()?;
        store.resolve_group(id)?;
        store.set_state("active_group_id", id)
    }
    pub fn resolve_session(&self, value: &str) -> Result<ConversationSummary> {
        self.store()?.resolve_conversation(value)
    }
    pub fn sessions(&self, group: Option<&str>) -> Result<Vec<ConversationSummary>> {
        self.store()?.list_conversations(group)
    }
    pub fn search(&self, keyword: &str) -> Result<Vec<ConversationSummary>> {
        self.store()?.search(keyword)
    }

    pub fn workspace(&self) -> Result<Workspace> {
        let store = self.store()?;
        let groups = store
            .list_groups(false)?
            .into_iter()
            .map(group_dto)
            .collect();
        let sessions = store
            .list_conversations(None)?
            .into_iter()
            .map(|s| {
                Ok(SessionSummary {
                    message_count: store.message_count(&s.id)?,
                    id: s.id,
                    group_id: s.group_id,
                    title: s.title,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let active_session_id = store
            .get_state("active_conversation_id")?
            .filter(|id| sessions.iter().any(|s| &s.id == id));
        Ok(Workspace {
            groups,
            sessions,
            active_session_id,
        })
    }

    pub fn history(&self, id: &str) -> Result<Vec<StoredMessage>> {
        let store = self.store()?;
        store.conversation_by_id(id)?;
        store.load_messages(id, false)
    }

    pub fn messages(&self, id: Option<&str>) -> Result<Vec<Message>> {
        let store = self.store()?;
        let target = id
            .map(str::to_owned)
            .or(store.get_state("active_conversation_id")?);
        let Some(target) = target else {
            return Ok(Vec::new());
        };
        store.conversation_by_id(&target)?;
        Ok(store
            .load_messages(&target, false)?
            .into_iter()
            .filter(|m| m.role != "assistant" || !m.content.is_empty() || m.status == "complete")
            .map(|m| Message {
                role: m.role,
                content: m.content,
            })
            .collect())
    }

    pub fn create_group(&self, input: NamedInput) -> Result<Group> {
        Ok(group_dto(self.store()?.create_group(&input.name)?))
    }

    pub fn rename_group(&self, input: IdNamedInput) -> Result<Group> {
        let store = self.store()?;
        store.rename_group(&input.id, &input.name)?;
        Ok(group_dto(store.group_by_id(&input.id)?))
    }

    pub fn delete_group(&self, id: &str) -> Result<()> {
        let _chat = self.chat_guard()?;
        self.store()?.delete_group(id)
    }

    pub fn archive_group(&self, id: &str) -> Result<()> {
        let _chat = self.chat_guard()?;
        self.store()?.archive_group(id)
    }

    pub fn create_session(&self, input: SessionCreateInput) -> Result<CreatedSession> {
        let store = self.store()?;
        let session = store.create_conversation(&input.group_id, &input.title, "")?;
        store.set_state("active_conversation_id", &session.id)?;
        store.set_state("active_group_id", &session.group_id)?;
        Ok(CreatedSession {
            created: true,
            session_id: session.id,
        })
    }

    pub fn select_session(&self, id: &str) -> Result<()> {
        let store = self.store()?;
        let session = store.conversation_by_id(id)?;
        store.set_state("active_conversation_id", id)?;
        store.set_state("active_group_id", &session.group_id)
    }

    pub fn rename_session(&self, input: IdNamedInput) -> Result<Session> {
        {
            let store = self.store()?;
            store.rename_conversation(&input.id, &input.name)?;
        }
        let session = self.resolve_session(&input.id)?;
        Ok(Session {
            id: session.id.clone(),
            group_id: session.group_id,
            title: session.title,
            messages: self.messages(Some(&session.id))?,
        })
    }

    pub fn move_session(&self, id: &str, group_id: &str) -> Result<()> {
        let _chat = self.chat_guard()?;
        self.store()?.move_conversation(id, group_id)
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
        if let Some(id) = target
            .session_id
            .or(store.get_state("active_conversation_id")?)
        {
            store.clear_conversation(&id)?;
        }
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
            let mut config = store.provider(input.provider_id.as_deref())?;
            if let Some(model) = input.model.filter(|m| !m.trim().is_empty()) {
                config.model = model.trim().to_owned();
            }
            config.validate()?;
            let id = match input
                .session_id
                .or(store.get_state("active_conversation_id")?)
            {
                Some(id) => id,
                None => {
                    let session =
                        store.create_conversation(&store.ungrouped_id()?, "", &config.model)?;
                    store.set_state("active_conversation_id", &session.id)?;
                    session.id
                }
            };
            let session = store.conversation_by_id(&id)?;
            store.resolve_group(&session.group_id)?;
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

fn group_dto(group: GroupSummary) -> Group {
    Group {
        id: group.id,
        name: group.name,
        is_default: group.is_system,
    }
}

fn provider_config(input: &ProviderInput) -> Result<(String, Config)> {
    let config = Config {
        base_url: input.base_url.trim().trim_end_matches('/').to_owned(),
        api_key: input.api_key.trim().to_owned(),
        model: input.model.trim().to_owned(),
    };
    if config.base_url.is_empty() || config.api_key.is_empty() {
        bail!("API 地址和 API Key 不能为空");
    }
    let name = if input.name.trim().is_empty() {
        provider_name_from_url(&config.base_url)
    } else {
        input.name.trim().to_owned()
    };
    Ok((name, config))
}

fn provider_name_from_url(base_url: &str) -> String {
    let host = base_url
        .trim()
        .trim_end_matches('/')
        .split_once("://")
        .map(|(_, value)| value)
        .unwrap_or(base_url.trim().trim_end_matches('/'))
        .split('/')
        .next()
        .unwrap_or_default()
        .rsplit('@')
        .next()
        .unwrap_or_default()
        .split(':')
        .next()
        .unwrap_or_default();
    let labels = host
        .split('.')
        .filter(|label| !label.is_empty())
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
                .create_conversation(&store.ungrouped_id().unwrap(), "fixture", "model")
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
