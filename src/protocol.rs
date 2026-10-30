//! 客户端与 Runtime 共用的请求和响应类型。
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReasoningSource {
    Builtin,
    Provider,
    Custom,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ModelDescriptor {
    pub id: String,
    pub reasoning_efforts: Vec<String>,
    pub reasoning_source: ReasoningSource,
}

#[derive(Debug, Serialize)]
pub(crate) struct ProviderSummary {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub api_key_configured: bool,
    pub active: bool,
    pub models: Vec<ModelDescriptor>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderInput {
    pub id: Option<String>,
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    pub models: Vec<ModelDescriptor>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderModelsInput {
    pub id: Option<String>,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Project {
    pub id: String,
    pub name: String,
    pub directory: String,
}

#[derive(Serialize)]
pub(crate) struct Workspace {
    pub projects: Vec<Project>,
    pub sessions: Vec<SessionSummary>,
    pub active_session_id: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct SessionSummary {
    pub id: String,
    pub project_id: Option<String>,
    pub title: String,
    pub message_count: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProjectCreateInput {
    pub directory: String,
    #[serde(default)]
    pub name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct IdInput {
    pub id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct IdNamedInput {
    pub id: String,
    pub name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SessionCreateInput {
    pub project_id: Option<String>,
    #[serde(default)]
    pub title: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SessionMoveInput {
    pub id: String,
    pub project_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SessionTarget {
    pub session_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChatInput {
    pub content: String,
    pub session_id: String,
    pub provider_id: String,
    pub model: String,
    pub reasoning_effort: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct CreatedSession {
    pub session_id: String,
}

#[derive(Serialize)]
pub(crate) struct ChatOutput {
    pub session_id: String,
    pub message: Message,
}
