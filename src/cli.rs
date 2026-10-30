//! CLI 只负责输入和展示，所有写入与模型请求均通过 AppServer。
use crate::app_server::AppServer;
use crate::protocol::*;
use anyhow::{Context, Result, bail};
use std::io::{self, Write};

struct Selection {
    project: Option<String>,
    session: Option<String>,
    title: String,
    model: Option<String>,
    reasoning: Option<String>,
}

pub(crate) fn run(app: &AppServer) -> Result<()> {
    let workspace = app.workspace()?;
    let project = app.default_project()?.map(|item| item.id);
    let session = workspace.active_session_id.filter(|id| {
        workspace
            .sessions
            .iter()
            .any(|item| &item.id == id && item.project_id == project)
    });
    let mut selected = Selection {
        project,
        session,
        title: String::new(),
        model: None,
        reasoning: None,
    };
    println!("xi CLI 已启动。输入 /help 查看命令，/exit 退出。\n");
    loop {
        print!("You: ");
        io::stdout().flush()?;
        let mut input = String::new();
        if io::stdin().read_line(&mut input)? == 0 {
            break;
        }
        let input = input.trim();
        if input.is_empty() {
            continue;
        }
        if input.starts_with('/') {
            match command(app, &mut selected, input) {
                Ok(true) => break,
                Ok(false) => {}
                Err(error) => eprintln!("命令执行失败：{error:#}"),
            }
        } else if let Err(error) = send(app, &mut selected, input) {
            eprintln!("\n调用失败：{error:#}");
        }
    }
    println!("会话结束，消息已保存到 SQLite。");
    Ok(())
}

fn send(app: &AppServer, selected: &mut Selection, content: &str) -> Result<()> {
    let providers = app.providers()?;
    let provider = providers
        .iter()
        .find(|item| item.active)
        .context("请先在应用端配置模型服务")?;
    let model = match &selected.model {
        Some(id) => provider
            .models
            .iter()
            .find(|model| &model.id == id)
            .context("所选模型不存在，请用 /models 检查")?,
        None => provider.models.first().context("请先获取并保存模型列表")?,
    };
    if selected.session.is_none() {
        selected.session = Some(
            app.create_session(SessionCreateInput {
                project_id: selected.project.clone(),
                title: selected.title.clone(),
            })?
            .session_id,
        );
    }
    println!("\nAssistant:");
    app.chat_stream(
        ChatInput {
            content: content.to_owned(),
            session_id: current(selected)?.to_owned(),
            provider_id: provider.id.clone(),
            model: model.id.clone(),
            reasoning_effort: selected.reasoning.clone(),
        },
        |delta| {
            print!("{delta}");
            io::stdout().flush().context("无法刷新终端输出")
        },
    )?;
    println!("\n");
    Ok(())
}

fn current(selected: &Selection) -> Result<&str> {
    selected
        .session
        .as_deref()
        .context("当前没有会话，请先发送消息或使用 /use")
}

fn select_project(app: &AppServer, selected: &mut Selection, id: Option<String>) -> Result<()> {
    app.select_project(id.as_deref())?;
    selected.session = app
        .sessions(None)?
        .into_iter()
        .find(|session| session.project_id == id)
        .map(|item| item.id);
    selected.project = id;
    selected.title.clear();
    if let Some(id) = &selected.session {
        app.select_session(id)?;
    }
    Ok(())
}

fn command(app: &AppServer, selected: &mut Selection, line: &str) -> Result<bool> {
    let (name, argument) = line
        .trim_start_matches('/')
        .split_once(char::is_whitespace)
        .unwrap_or((line.trim_start_matches('/'), ""));
    let argument = argument.trim();
    match name.to_ascii_lowercase().as_str() {
        "exit" => return Ok(true),
        "help" => println!(
            "命令：\n/projects、/project-open <绝对目录>、/project-use <ID或名称或->\n/project-rename <名称>、/project-move <会话ID> <项目ID或->\n/project-archive [项目]、/project-remove [项目]\n/providers、/provider-use <ID>、/models、/model <ID>、/reasoning <auto或模型支持的值>\n/new [标题]、/sessions [--all]、/use <ID或标题>、/rename <标题>\n/clear、/archive、/search <关键词>、/export <路径>、/exit"
        ),
        "projects" => {
            for project in app.projects()? {
                println!(
                    "{}  {}  {}（{} 个会话）",
                    project.id, project.name, project.directory, project.conversation_count
                );
            }
        }
        "project-open" => {
            let project = app.create_project(ProjectCreateInput {
                directory: argument.to_owned(),
                name: String::new(),
            })?;
            select_project(app, selected, Some(project.id))?;
            println!("已打开项目：{}", project.name);
        }
        "project-use" => {
            let id = if argument == "-" {
                None
            } else {
                Some(app.resolve_project(argument)?.id)
            };
            select_project(app, selected, id)?;
            println!("已切换项目。");
        }
        "project-rename" => {
            app.rename_project(IdNamedInput {
                id: selected.project.clone().context("当前是独立聊天")?,
                name: argument.to_owned(),
            })?;
            println!("项目已重命名。");
        }
        "project-move" => {
            let (session, project) = argument
                .split_once(char::is_whitespace)
                .context("用法：/project-move <会话ID> <项目ID或->")?;
            let session = app.resolve_session(session)?;
            let project = if project.trim() == "-" {
                None
            } else {
                Some(app.resolve_project(project.trim())?.id)
            };
            app.move_session(&session.id, project.as_deref())?;
            if selected.session.as_deref() == Some(&session.id) {
                selected.project = project;
                app.select_session(&session.id)?;
            }
            println!("会话已移动。");
        }
        "project-archive" | "project-remove" => {
            let id = if argument.is_empty() {
                selected.project.clone().context("当前是独立聊天")?
            } else {
                app.resolve_project(argument)?.id
            };
            if name.eq_ignore_ascii_case("project-archive") {
                app.archive_project(&id)?;
            } else {
                app.delete_project(&id)?;
            }
            if selected.project.as_deref() == Some(&id) {
                select_project(app, selected, None)?;
            }
            println!("项目操作完成，本地文件夹未删除。");
        }
        "providers" => {
            for provider in app.providers()? {
                println!(
                    "{}  {}{}",
                    provider.id,
                    provider.name,
                    if provider.active { "（当前）" } else { "" }
                );
            }
        }
        "provider-use" => {
            app.select_provider(argument)?;
            selected.model = None;
            selected.reasoning = None;
            println!("已切换模型服务。");
        }
        "models" => {
            let providers = app.providers()?;
            let provider = providers
                .iter()
                .find(|provider| provider.active)
                .context("请先配置模型服务")?;
            for model in &provider.models {
                println!(
                    "{}  推理选项：{}",
                    model.id,
                    model.reasoning_efforts.join(", ")
                );
            }
        }
        "model" => {
            let providers = app.providers()?;
            let provider = providers
                .iter()
                .find(|provider| provider.active)
                .context("请先配置模型服务")?;
            if !provider.models.iter().any(|model| model.id == argument) {
                bail!("所选模型不存在");
            }
            selected.model = Some(argument.to_owned());
            selected.reasoning = None;
        }
        "reasoning" => {
            if argument == "auto" {
                selected.reasoning = None;
            } else {
                let providers = app.providers()?;
                let provider = providers
                    .iter()
                    .find(|provider| provider.active)
                    .context("请先配置模型服务")?;
                let model = selected
                    .model
                    .as_ref()
                    .and_then(|id| provider.models.iter().find(|model| &model.id == id))
                    .or_else(|| provider.models.first())
                    .context("请先获取模型")?;
                if !model
                    .reasoning_efforts
                    .iter()
                    .any(|effort| effort == argument)
                {
                    bail!("所选模型不支持推理强度：{argument}");
                }
                selected.reasoning = Some(argument.to_owned());
            }
        }
        "new" => {
            selected.session = None;
            selected.title = argument.to_owned();
            println!("已打开新会话草稿，发送首条消息后保存。");
        }
        "sessions" => {
            for session in app
                .sessions(None)?
                .into_iter()
                .filter(|session| argument == "--all" || session.project_id == selected.project)
            {
                println!(
                    "{}  {}（模型：{}）",
                    session.id, session.title, session.model
                );
            }
        }
        "use" => {
            let session = app.resolve_session(argument)?;
            selected.project = session.project_id;
            selected.session = Some(session.id.clone());
            selected.title.clear();
            app.select_session(&session.id)?;
            println!("已切换到会话：{}", session.title);
        }
        "rename" => {
            app.rename_session(IdNamedInput {
                id: current(selected)?.to_owned(),
                name: argument.to_owned(),
            })?;
            println!("会话已重命名。");
        }
        "clear" => {
            app.reset(SessionTarget {
                session_id: current(selected)?.to_owned(),
            })?;
            println!("会话消息已清空。");
        }
        "archive" => {
            app.archive_session(current(selected)?)?;
            selected.session = None;
            println!("会话已归档。");
        }
        "search" => {
            for session in app.search(argument)? {
                println!(
                    "{}  {}（模型：{}）",
                    session.id, session.title, session.model
                );
            }
        }
        "export" => {
            if argument.is_empty() {
                bail!("用法：/export <路径>");
            }
            std::fs::write(argument, app.export_session(current(selected)?)?)?;
            println!("已导出当前会话。");
        }
        _ => bail!("未知命令：/{name}，输入 /help 查看帮助"),
    }
    Ok(false)
}
