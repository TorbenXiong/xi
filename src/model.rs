//! 模型适配同时支持 CLI 流式输出和 HTTP 完整响应。
use crate::protocol::Message;
use anyhow::{Context, Result, bail};
use reqwest::header::CONTENT_TYPE;
use serde::{Deserialize, Serialize};
use std::time::Duration;

const MAX_MODEL_RESPONSE: usize = 8 * 1024 * 1024;

#[derive(Clone, Default)]
pub(crate) struct Config {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

impl Config {
    pub fn validate(&self) -> Result<()> {
        if self.base_url.trim().is_empty()
            || self.api_key.trim().is_empty()
            || self.model.trim().is_empty()
        {
            bail!("配置中的 base_url、api_key 和 model 都不能为空");
        }
        Ok(())
    }
}

#[derive(Debug)]
pub(crate) struct ModelError {
    message: String,
    pub partial_content: String,
}
impl ModelError {
    fn partial(error: impl std::fmt::Display, partial: &str) -> Self {
        Self {
            message: error.to_string(),
            partial_content: partial.to_owned(),
        }
    }
}
impl std::fmt::Display for ModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for ModelError {}
impl From<anyhow::Error> for ModelError {
    fn from(error: anyhow::Error) -> Self {
        Self::partial(error, "")
    }
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<String>,
}
#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}
#[derive(Deserialize)]
struct Choice {
    message: Message,
}
#[derive(Deserialize)]
struct StreamResponse {
    choices: Vec<StreamChoice>,
}
#[derive(Deserialize)]
struct StreamChoice {
    #[serde(default)]
    delta: Delta,
    finish_reason: Option<String>,
}
#[derive(Default, Deserialize)]
struct Delta {
    content: Option<String>,
}
#[derive(Deserialize)]
struct ModelsResponse {
    data: Vec<ModelItem>,
}
#[derive(Deserialize)]
struct ModelItem {
    id: String,
}

pub(crate) struct ModelRuntime {
    client: reqwest::Client,
    runtime: tokio::runtime::Runtime,
}
impl ModelRuntime {
    pub fn new() -> Result<Self> {
        Ok(Self {
            runtime: tokio::runtime::Runtime::new().context("无法创建应用运行时")?,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(120))
                .build()
                .context("无法创建 HTTP 客户端")?,
        })
    }

    pub fn complete_with(
        &self,
        config: &Config,
        messages: &[Message],
        reasoning: Option<String>,
        stream: bool,
        on_delta: impl FnMut(&str) -> Result<()>,
    ) -> std::result::Result<String, ModelError> {
        // Future 构造也必须进入 Tokio，不能提前执行 reqwest::send。
        self.runtime.block_on(async {
            call_model(&self.client, config, messages, reasoning, stream, on_delta).await
        })
    }

    pub fn models(&self, config: &Config) -> Result<Vec<String>> {
        self.runtime.block_on(async {
            let response = self
                .client
                .get(format!("{}/models", config.base_url.trim_end_matches('/')))
                .bearer_auth(&config.api_key)
                .send()
                .await
                .context("获取模型列表失败")?;
            let status = response.status();
            let body = read_body(response).await?;
            if !status.is_success() {
                bail!("模型服务返回 HTTP {}：{}", status, preview(&body));
            }
            let response: ModelsResponse =
                serde_json::from_str(&body).context("模型列表不是预期 JSON")?;
            let mut models = response
                .data
                .into_iter()
                .map(|item| item.id)
                .collect::<Vec<_>>();
            models.sort_unstable();
            models.dedup();
            Ok(models)
        })
    }
}

async fn read_body(mut response: reqwest::Response) -> Result<String> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.context("读取模型响应失败")? {
        if body.len() + chunk.len() > MAX_MODEL_RESPONSE {
            bail!("模型响应超过 8 MB 上限");
        }
        body.extend_from_slice(&chunk);
    }
    String::from_utf8(body).context("模型响应不是 UTF-8")
}

async fn call_model(
    client: &reqwest::Client,
    config: &Config,
    messages: &[Message],
    reasoning_effort: Option<String>,
    stream: bool,
    mut on_delta: impl FnMut(&str) -> Result<()>,
) -> std::result::Result<String, ModelError> {
    config.validate()?;
    let request = ChatRequest {
        model: config.model.clone(),
        messages: messages.to_vec(),
        stream,
        reasoning_effort,
    };
    let mut response = client
        .post(format!(
            "{}/chat/completions",
            config.base_url.trim_end_matches('/')
        ))
        .bearer_auth(&config.api_key)
        .json(&request)
        .send()
        .await
        .context("调用模型接口失败，请检查网络和 base_url")?;
    let status = response.status();
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !status.is_success() {
        let body = read_body(response).await?;
        return Err(ModelError::partial(
            format!("模型接口返回 HTTP {status}：{}", preview(&body)),
            "",
        ));
    }
    if content_type.starts_with("text/event-stream") {
        return read_stream_response(&mut response, on_delta).await;
    }
    let body = read_body(response).await?;
    let parsed: ChatResponse = serde_json::from_str(&body)
        .with_context(|| format!("模型响应不是预期 JSON：{}", preview(&body)))?;
    let answer = parsed
        .choices
        .into_iter()
        .next()
        .context("模型响应中没有 choices[0].message.content")?
        .message
        .content;
    on_delta(&answer).map_err(|error| ModelError::partial(error, &answer))?;
    Ok(answer)
}

#[derive(Debug, PartialEq)]
enum StreamEvent {
    Delta(String),
    Finished(Option<String>),
    Empty,
}

async fn read_stream_response(
    response: &mut reqwest::Response,
    mut on_delta: impl FnMut(&str) -> Result<()>,
) -> std::result::Result<String, ModelError> {
    let mut pending = Vec::new();
    let mut answer = String::new();
    let mut size = 0;
    let mut finished = false;
    while let Some(chunk) = response
        .chunk()
        .await
        .context("读取模型流式响应失败")
        .map_err(|e| ModelError::partial(e, &answer))?
    {
        size += chunk.len();
        if size > MAX_MODEL_RESPONSE {
            return Err(ModelError::partial("模型响应超过 8 MB 上限", &answer));
        }
        pending.extend_from_slice(&chunk);
        while let Some((end, separator_len)) = find_sse_event_end(&pending) {
            let event = pending.drain(..end + separator_len).collect::<Vec<_>>();
            let event =
                parse_sse_event(&event[..end]).map_err(|e| ModelError::partial(e, &answer))?;
            let delta = match event {
                StreamEvent::Delta(delta) => Some(delta),
                StreamEvent::Finished(delta) => {
                    finished = true;
                    delta
                }
                StreamEvent::Empty => None,
            };
            if let Some(delta) = delta {
                answer.push_str(&delta);
                on_delta(&delta).map_err(|e| ModelError::partial(e, &answer))?;
            }
            if finished {
                return Ok(answer);
            }
        }
    }
    // 即使已有部分文本，缺少结束事件也不能当作完整成功。
    if !pending.is_empty() {
        match parse_sse_event(&pending).map_err(|e| ModelError::partial(e, &answer))? {
            StreamEvent::Finished(delta) => {
                if let Some(delta) = delta {
                    answer.push_str(&delta);
                    on_delta(&delta).map_err(|e| ModelError::partial(e, &answer))?;
                }
                finished = true;
            }
            StreamEvent::Delta(delta) => {
                answer.push_str(&delta);
                on_delta(&delta).map_err(|e| ModelError::partial(e, &answer))?;
            }
            StreamEvent::Empty => {}
        }
    }
    if !finished {
        return Err(ModelError::partial("模型响应在结束事件前中断", &answer));
    }
    Ok(answer)
}

fn find_sse_event_end(buffer: &[u8]) -> Option<(usize, usize)> {
    let lf = buffer.windows(2).position(|w| w == b"\n\n");
    let crlf = buffer.windows(4).position(|w| w == b"\r\n\r\n");
    match (lf, crlf) {
        (Some(lf), Some(crlf)) if crlf < lf => Some((crlf, 4)),
        (Some(lf), _) => Some((lf, 2)),
        (None, Some(crlf)) => Some((crlf, 4)),
        _ => None,
    }
}

fn parse_sse_event(event: &[u8]) -> Result<StreamEvent> {
    let event = std::str::from_utf8(event).context("模型流式响应包含无效 UTF-8")?;
    let data = event
        .lines()
        .filter_map(|line| {
            line.strip_prefix("data:")
                .map(|s| s.strip_prefix(' ').unwrap_or(s))
        })
        .collect::<Vec<_>>()
        .join("\n");
    if data.is_empty() {
        return Ok(StreamEvent::Empty);
    }
    if data == "[DONE]" {
        return Ok(StreamEvent::Finished(None));
    }
    let response: StreamResponse =
        serde_json::from_str(&data).context("模型流式事件不是预期 JSON")?;
    let Some(choice) = response.choices.into_iter().next() else {
        return Ok(StreamEvent::Empty);
    };
    if choice.finish_reason.is_some() {
        return Ok(StreamEvent::Finished(choice.delta.content));
    }
    Ok(choice
        .delta
        .content
        .map(StreamEvent::Delta)
        .unwrap_or(StreamEvent::Empty))
}

fn preview(body: &str) -> String {
    let mut result = body.chars().take(500).collect::<String>();
    if body.chars().count() > 500 {
        result.push_str("...");
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn config_validation_rejects_missing_values() {
        assert!(Config::default().validate().is_err());
    }
    #[test]
    fn preview_limits_long_responses() {
        assert_eq!(preview(&"a".repeat(501)).len(), 503);
    }
    #[test]
    fn finds_lf_and_crlf_event_boundaries() {
        assert_eq!(find_sse_event_end(b"data: one\n\ndata: two"), Some((9, 2)));
        assert_eq!(
            find_sse_event_end(b"data: one\r\n\r\ndata: two"),
            Some((9, 4))
        );
        assert_eq!(find_sse_event_end(b"data: incomplete"), None);
    }
    #[test]
    fn parses_content_and_done_events() {
        assert_eq!(
            parse_sse_event(br#"data: {"choices":[{"delta":{"content":"\u4f60\u597d"}}]}"#)
                .unwrap(),
            StreamEvent::Delta("你好".into())
        );
        assert_eq!(
            parse_sse_event(b"data: [DONE]").unwrap(),
            StreamEvent::Finished(None)
        );
    }
}
