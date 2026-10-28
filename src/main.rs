use std::io::{self, Write};
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use reqwest::header::CONTENT_TYPE;
use serde::{Deserialize, Serialize};

mod storage;

use storage::{ConversationSummary, GroupSummary, Store, default_database_path};

#[derive(Debug)]
struct ModelError {
    message: String,
    partial_content: String,
}

impl ModelError {
    fn with_partial(error: impl std::fmt::Display, partial_content: &str) -> Self {
        Self {
            message: error.to_string(),
            partial_content: partial_content.to_owned(),
        }
    }
}

impl std::fmt::Display for ModelError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ModelError {}

impl From<anyhow::Error> for ModelError {
    fn from(error: anyhow::Error) -> Self {
        Self::with_partial(error, "")
    }
}

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

#[derive(Debug, Deserialize)]
struct StreamResponse {
    choices: Vec<StreamChoice>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    delta: Delta,
}

#[derive(Debug, Default, Deserialize)]
struct Delta {
    content: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = load_config(Path::new("config/local.toml"))?;
    let client = reqwest::Client::new();
    let database_path = default_database_path()?;
    let mut store = Store::open(&database_path)?;
    let recovered = store.mark_stale_runs_interrupted()?;
    if recovered > 0 {
        eprintln!("已将 {} 条未完成的请求标记为中断。", recovered);
    }

    let mut active_group = restore_group(&store)?;
    let mut active_conversation = restore_conversation(&store, &active_group.id, &config.model)?;
    store.set_state("active_group_id", &active_group.id)?;
    store.set_state("active_conversation_id", &active_conversation.id)?;

    println!("数据库：{}", database_path.display());
    println!(
        "当前分组：{}；当前会话：{}",
        active_group.name, active_conversation.title
    );
    println!("输入 /help 查看会话和分组命令。\n");

    loop {
        let Some(user_input) = read_user_input()? else {
            break;
        };

        if user_input.starts_with('/') {
            match handle_command(
                &user_input,
                &mut store,
                &config.model,
                &mut active_group,
                &mut active_conversation,
            ) {
                Ok(true) => break,
                Ok(false) => {}
                Err(error) => eprintln!("命令执行失败：{error:#}"),
            }
            continue;
        }

        let run = match store.start_run(&active_conversation.id, &user_input, &config.model) {
            Ok(run) => run,
            Err(error) => {
                eprintln!("无法保存用户消息：{error:#}");
                continue;
            }
        };
        let messages = store
            .load_messages(&active_conversation.id, true)?
            .into_iter()
            .map(|message| Message {
                role: message.role,
                content: message.content,
            })
            .collect::<Vec<_>>();

        print!("\nAssistant:\n");
        io::stdout().flush().context("无法刷新终端输出")?;
        match call_model(&client, &config, &messages).await {
            Ok(response) => {
                store.complete_run(&run, &response)?;
                active_conversation = store.conversation_by_id(&active_conversation.id)?;
                store.set_state("active_conversation_id", &active_conversation.id)?;
                println!("\n");
            }
            Err(error) => {
                let error_text = format!("{error:#}");
                let save_result = if error.partial_content.is_empty() {
                    store.fail_run(&run, &error_text)
                } else {
                    store.interrupt_run(&run, &error.partial_content, &error_text)
                };
                if let Err(save_error) = save_result {
                    eprintln!("\n模型调用失败，且无法保存失败状态：{save_error:#}");
                } else {
                    eprintln!("\n模型调用失败：{error_text}");
                }
            }
        }
    }

    let message_count = store.load_messages(&active_conversation.id, false)?.len();
    println!("会话结束。当前会话已保存 {} 条消息。", message_count);

    Ok(())
}

fn restore_group(store: &Store) -> Result<GroupSummary> {
    if let Some(id) = store.get_state("active_group_id")?
        && let Ok(group) = store.resolve_group(&id)
    {
        return Ok(group);
    }
    store
        .resolve_group(storage::UNGROUPED_NAME)
        .context("无法恢复默认分组")
}

fn restore_conversation(store: &Store, group_id: &str, model: &str) -> Result<ConversationSummary> {
    if let Some(id) = store.get_state("active_conversation_id")?
        && let Ok(conversation) = store.conversation_by_id(&id)
        && conversation.group_id == group_id
    {
        return Ok(conversation);
    }
    if let Some(conversation) = store.list_conversations(Some(group_id))?.into_iter().next() {
        return Ok(conversation);
    }
    store.create_conversation(group_id, "新会话", model)
}

fn handle_command(
    input: &str,
    store: &mut Store,
    model: &str,
    active_group: &mut GroupSummary,
    active_conversation: &mut ConversationSummary,
) -> Result<bool> {
    let command_line = input.trim_start_matches('/').trim();
    let mut parts = command_line.splitn(2, char::is_whitespace);
    let command = parts.next().unwrap_or_default().to_ascii_lowercase();
    let argument = parts.next().unwrap_or_default().trim();

    match command.as_str() {
        "exit" | "quit" => return Ok(true),
        "help" => print_help(),
        "groups" => print_groups(store)?,
        "group-new" => {
            let group = store.create_group(argument)?;
            let conversation = restore_conversation(store, &group.id, model)?;
            *active_group = group.clone();
            *active_conversation = conversation.clone();
            store.set_state("active_group_id", &group.id)?;
            store.set_state("active_conversation_id", &conversation.id)?;
            println!("已创建并切换到分组：{}", group.name);
        }
        "group-use" => {
            let group = store.resolve_group(argument)?;
            let conversation = restore_conversation(store, &group.id, model)?;
            *active_group = group.clone();
            *active_conversation = conversation.clone();
            store.set_state("active_group_id", &group.id)?;
            store.set_state("active_conversation_id", &conversation.id)?;
            println!("已切换到分组：{}；会话：{}", group.name, conversation.title);
        }
        "group-rename" => {
            store.rename_group(&active_group.id, argument)?;
            *active_group = store.group_by_id(&active_group.id)?;
            println!("分组已重命名为：{}", active_group.name);
        }
        "group-move" => {
            let (conversation_ref, group_ref) = argument
                .split_once(char::is_whitespace)
                .ok_or_else(|| anyhow!("用法：/group-move <会话 ID 或名称> <分组 ID 或名称>"))?;
            let conversation = store.resolve_conversation(conversation_ref.trim())?;
            let group = store.resolve_group(group_ref.trim())?;
            store.move_conversation(&conversation.id, &group.id)?;
            if conversation.id == active_conversation.id {
                *active_group = group.clone();
                *active_conversation = store.conversation_by_id(&conversation.id)?;
                store.set_state("active_group_id", &group.id)?;
            }
            println!("会话已移动到分组：{}", group.name);
        }
        "group-archive" => {
            let group = if argument.is_empty() {
                active_group.clone()
            } else {
                store.resolve_group(argument)?
            };
            store.archive_group(&group.id)?;
            *active_group = store.resolve_group(storage::UNGROUPED_NAME)?;
            *active_conversation = restore_conversation(store, &active_group.id, model)?;
            store.set_state("active_group_id", &active_group.id)?;
            store.set_state("active_conversation_id", &active_conversation.id)?;
            println!("分组已归档，已切换到：{}", active_group.name);
        }
        "group-delete" => {
            let group = if argument.is_empty() {
                active_group.clone()
            } else {
                store.resolve_group(argument)?
            };
            store.delete_group(&group.id)?;
            *active_group = store.resolve_group(storage::UNGROUPED_NAME)?;
            *active_conversation = restore_conversation(store, &active_group.id, model)?;
            store.set_state("active_group_id", &active_group.id)?;
            store.set_state("active_conversation_id", &active_conversation.id)?;
            println!("分组已删除，会话已移动到：{}", active_group.name);
        }
        "new" => {
            *active_conversation = store.create_conversation(&active_group.id, argument, model)?;
            store.set_state("active_conversation_id", &active_conversation.id)?;
            println!("已创建会话：{}", active_conversation.title);
        }
        "sessions" => {
            let all = argument == "--all";
            print_conversations(store, if all { None } else { Some(&active_group.id) })?;
        }
        "use" => {
            let conversation = store.resolve_conversation(argument)?;
            *active_group = store.group_by_id(&conversation.group_id)?;
            *active_conversation = conversation.clone();
            store.set_state("active_group_id", &active_group.id)?;
            store.set_state("active_conversation_id", &active_conversation.id)?;
            println!(
                "已切换到会话：{}（分组：{}）",
                conversation.title, active_group.name
            );
        }
        "rename" => {
            store.rename_conversation(&active_conversation.id, argument)?;
            *active_conversation = store.conversation_by_id(&active_conversation.id)?;
            println!("会话已重命名为：{}", active_conversation.title);
        }
        "clear" => {
            store.clear_conversation(&active_conversation.id)?;
            println!("当前会话消息已清空。");
        }
        "archive" | "delete" => {
            store.archive_conversation(&active_conversation.id)?;
            *active_conversation = restore_conversation(store, &active_group.id, model)?;
            store.set_state("active_conversation_id", &active_conversation.id)?;
            println!("当前会话已归档，已切换到：{}", active_conversation.title);
        }
        "search" => {
            print_search(store, argument)?;
        }
        "export" => {
            export_conversation(store, &active_conversation.id, argument)?;
            println!("已导出当前会话。");
        }
        _ => bail!("未知命令：/{command}，输入 /help 查看帮助"),
    }
    Ok(false)
}

fn print_help() {
    println!(
        "命令：\n\
         /groups、/group-new <名称>、/group-use <ID或名称>、/group-rename <名称>\n\
         /group-move <会话> <分组>、/group-archive [分组]、/group-delete [分组]\n\
         /new [标题]、/sessions [--all]、/use <ID或标题>、/rename <标题>\n\
         /clear、/archive、/search <关键词>、/export <路径>、/exit"
    );
}

fn print_groups(store: &Store) -> Result<()> {
    for group in store.list_groups(false)? {
        println!(
            "{}  {}（{} 个会话，更新时间 {}）",
            group.id, group.name, group.conversation_count, group.updated_at
        );
    }
    Ok(())
}

fn print_conversations(store: &Store, group_id: Option<&str>) -> Result<()> {
    for conversation in store.list_conversations(group_id)? {
        let group = store.group_by_id(&conversation.group_id)?;
        println!(
            "{}  [{}] {}（模型：{}，更新时间 {}）",
            conversation.id,
            group.name,
            conversation.title,
            conversation.model,
            conversation.updated_at
        );
    }
    Ok(())
}

fn print_search(store: &Store, keyword: &str) -> Result<()> {
    for conversation in store.search(keyword)? {
        let group = store.group_by_id(&conversation.group_id)?;
        println!(
            "{}  [{}] {}（模型：{}）",
            conversation.id, group.name, conversation.title, conversation.model
        );
    }
    Ok(())
}

fn export_conversation(store: &Store, conversation_id: &str, path: &str) -> Result<()> {
    if path.is_empty() {
        bail!("用法：/export <路径>");
    }
    let conversation = store.conversation_by_id(conversation_id)?;
    let messages = store.load_messages(conversation_id, false)?;
    let mut output = format!("# {}\n\n", conversation.title);
    for message in messages {
        output.push_str(&format!(
            "## {} #{} ({})\n\n{}\n\n",
            message.role, message.sequence, message.status, message.content
        ));
        if let Some(error) = message.error {
            output.push_str(&format!("错误：{}\n\n", error));
        }
    }
    std::fs::write(path, output).with_context(|| format!("无法写入导出文件：{path}"))?;
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

async fn call_model(
    client: &reqwest::Client,
    config: &Config,
    messages: &[Message],
) -> std::result::Result<String, ModelError> {
    let endpoint = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
    let request = ChatRequest {
        model: config.model.clone(),
        messages: messages.to_vec(),
        stream: true,
    };

    let mut response = client
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
    if !status.is_success() {
        let body = response.text().await.context("读取模型响应失败")?;
        return Err(ModelError::with_partial(
            format!(
                "模型接口返回 HTTP {}（Content-Type: {}）：{}",
                status,
                content_type,
                preview(&body)
            ),
            "",
        ));
    }

    if !content_type
        .to_ascii_lowercase()
        .starts_with("text/event-stream")
    {
        let body = response.text().await.context("读取模型响应失败")?;
        let response: ChatResponse = serde_json::from_str(&body).with_context(|| {
            format!(
                "模型响应不是预期 JSON（HTTP {}，Content-Type: {}）：{}",
                status,
                content_type,
                preview(&body)
            )
        })?;
        let content = response
            .choices
            .into_iter()
            .next()
            .map(|choice| choice.message.content)
            .context("模型响应中没有 choices[0].message.content")?;
        print!("{}", content);
        io::stdout().flush().context("无法刷新终端输出")?;
        return Ok(content);
    }

    read_stream_response(&mut response).await
}

async fn read_stream_response(
    response: &mut reqwest::Response,
) -> std::result::Result<String, ModelError> {
    let mut pending = Vec::new();
    let mut answer = String::new();

    while let Some(chunk) = response
        .chunk()
        .await
        .context("读取模型流式响应失败")
        .map_err(|error| ModelError::with_partial(error, &answer))?
    {
        pending.extend_from_slice(&chunk);

        while let Some((end, separator_len)) = find_sse_event_end(&pending) {
            let event = pending.drain(..end).collect::<Vec<_>>();
            pending.drain(..separator_len);
            if let Some(delta) =
                parse_sse_event(&event).map_err(|error| ModelError::with_partial(error, &answer))?
            {
                print!("{}", delta);
                io::stdout()
                    .flush()
                    .context("无法刷新终端输出")
                    .map_err(|error| ModelError::with_partial(error, &answer))?;
                answer.push_str(&delta);
            }
        }
    }

    if !pending.is_empty() {
        if let Some(delta) =
            parse_sse_event(&pending).map_err(|error| ModelError::with_partial(error, &answer))?
        {
            print!("{}", delta);
            io::stdout()
                .flush()
                .context("无法刷新终端输出")
                .map_err(|error| ModelError::with_partial(error, &answer))?;
            answer.push_str(&delta);
        }
    }

    if answer.is_empty() {
        return Err(ModelError::with_partial(
            "模型流式响应中没有可显示的内容",
            "",
        ));
    }

    Ok(answer)
}

fn find_sse_event_end(buffer: &[u8]) -> Option<(usize, usize)> {
    let lf = buffer.windows(2).position(|window| window == b"\n\n");
    let crlf = buffer.windows(4).position(|window| window == b"\r\n\r\n");

    match (lf, crlf) {
        (Some(lf), Some(crlf)) if crlf < lf => Some((crlf, 4)),
        (Some(lf), _) => Some((lf, 2)),
        (None, Some(crlf)) => Some((crlf, 4)),
        (None, None) => None,
    }
}

fn parse_sse_event(event: &[u8]) -> Result<Option<String>> {
    let event = std::str::from_utf8(event).context("模型流式响应包含无效 UTF-8")?;
    let mut data = String::new();

    for line in event.lines() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if let Some(value) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(value.strip_prefix(' ').unwrap_or(value));
        }
    }

    if data.is_empty() || data == "[DONE]" {
        return Ok(None);
    }

    let response: StreamResponse = serde_json::from_str(&data)
        .with_context(|| format!("模型流式事件不是预期 JSON：{}", preview(&data)))?;
    Ok(response
        .choices
        .into_iter()
        .next()
        .and_then(|choice| choice.delta.content))
}

fn preview(body: &str) -> String {
    const MAX_CHARS: usize = 500;
    let mut result: String = body.chars().take(MAX_CHARS).collect();
    if body.chars().count() > MAX_CHARS {
        result.push_str("...");
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{find_sse_event_end, parse_sse_event};

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
        let event = br#"data: {"choices":[{"delta":{"content":"\u4f60\u597d"}}]}"#;
        assert_eq!(parse_sse_event(event).unwrap().as_deref(), Some("你好"));
        assert_eq!(parse_sse_event(b"data: [DONE]").unwrap(), None);
    }
}
