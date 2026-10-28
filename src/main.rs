use std::io::{self, Write};
use std::path::Path;

use anyhow::{Context, Result, bail};
use reqwest::header::CONTENT_TYPE;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
struct Config {
    base_url: String,
    api_key: String,
    model: String,
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    stream: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: Message,
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = load_config(Path::new("config/local.toml"))?;
    let mut messages = Vec::new();

    loop {
        let Some(user_input) = read_user_input()? else {
            break;
        };

        if matches!(user_input.as_str(), "/exit" | "/quit") {
            break;
        }

        messages.push(Message {
            role: "user".to_owned(),
            content: user_input,
        });

        let response = call_model(&config, &messages).await?;
        println!("\nAssistant:\n{}\n", response);

        messages.push(Message {
            role: "assistant".to_owned(),
            content: response,
        });
    }

    println!("会话结束。已保留 {} 条消息。", messages.len());

    Ok(())
}

fn load_config(path: &Path) -> Result<Config> {
    let text = std::fs::read_to_string(path).with_context(|| {
        format!(
            "无法读取配置文件 {}，请复制 config/example.toml 为 config/local.toml 并填写配置",
            path.display()
        )
    })?;

    let config: Config =
        toml::from_str(&text).with_context(|| format!("配置文件 {} 格式错误", path.display()))?;

    if config.base_url.trim().is_empty()
        || config.api_key.trim().is_empty()
        || config.model.trim().is_empty()
    {
        bail!("配置文件中的 base_url、api_key 和 model 都不能为空");
    }

    Ok(config)
}

fn read_user_input() -> Result<Option<String>> {
    print!("You: ");
    io::stdout().flush().context("无法刷新终端输出")?;

    let mut input = String::new();
    let bytes_read = io::stdin()
        .read_line(&mut input)
        .context("无法读取用户输入")?;

    if bytes_read == 0 {
        return Ok(None);
    }

    let input = input.trim().to_owned();
    if input.is_empty() {
        bail!("输入不能为空");
    }

    Ok(Some(input))
}

async fn call_model(config: &Config, messages: &[Message]) -> Result<String> {
    let endpoint = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
    let request = ChatRequest {
        model: config.model.clone(),
        messages: messages.to_vec(),
        stream: false,
    };

    let response = reqwest::Client::new()
        .post(endpoint)
        .bearer_auth(&config.api_key)
        .json(&request)
        .send()
        .await
        .context("调用模型接口失败，请检查网络和 base_url")?;

    let status = response.status();
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("<unknown>")
        .to_owned();
    let body = response.text().await.context("读取模型响应失败")?;

    if !status.is_success() {
        bail!(
            "模型接口返回 HTTP {}（Content-Type: {}）：{}",
            status,
            content_type,
            preview(&body)
        );
    }

    let response: ChatResponse = serde_json::from_str(&body).with_context(|| {
        format!(
            "模型响应不是预期 JSON（HTTP {}，Content-Type: {}）：{}",
            status,
            content_type,
            preview(&body)
        )
    })?;
    response
        .choices
        .into_iter()
        .next()
        .map(|choice| choice.message.content)
        .context("模型响应中没有 choices[0].message.content")
}

fn preview(body: &str) -> String {
    const MAX_CHARS: usize = 500;
    let mut result: String = body.chars().take(MAX_CHARS).collect();
    if body.chars().count() > MAX_CHARS {
        result.push_str("...");
    }
    result
}
