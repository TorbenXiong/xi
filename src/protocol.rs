//! 与界面、传输方式无关的应用请求和响应。
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ProviderSummary {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub model: String,
    pub api_key_configured: bool,
    pub active: bool,
    pub models: Vec<String>,
}

#[derive(Deserialize)]
pub(crate) struct ProviderInput {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: String,
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub models: Vec<String>,
}

#[derive(Default, Deserialize)]
pub(crate) struct ProviderModelsInput {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Group {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Session {
    pub id: String,
    pub group_id: String,
    pub title: String,
    pub messages: Vec<Message>,
}

#[derive(Serialize)]
pub(crate) struct Workspace {
    pub groups: Vec<Group>,
    pub sessions: Vec<SessionSummary>,
    pub active_session_id: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct SessionSummary {
    pub id: String,
    pub group_id: String,
    pub title: String,
    pub message_count: usize,
}

#[derive(Deserialize)]
pub(crate) struct NamedInput {
    pub name: String,
}

#[derive(Deserialize)]
pub(crate) struct IdInput {
    pub id: String,
}

#[derive(Deserialize)]
pub(crate) struct IdNamedInput {
    pub id: String,
    pub name: String,
}

#[derive(Deserialize)]
pub(crate) struct SessionCreateInput {
    pub group_id: String,
    #[serde(default)]
    pub title: String,
}

#[derive(Deserialize)]
pub(crate) struct SessionMoveInput {
    pub id: String,
    pub group_id: String,
}

#[derive(Default, Deserialize)]
pub(crate) struct SessionTarget {
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Default, Deserialize)]
pub(crate) struct ChatInput {
    pub content: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct CreatedSession {
    pub created: bool,
    pub session_id: String,
}

#[derive(Serialize)]
pub(crate) struct ChatOutput {
    pub session_id: String,
    pub message: Message,
}
