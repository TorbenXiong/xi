//! CLI 仅负责输入、展示和文件输出；所有会话操作和模型调用均通过 AppServer。
use crate::app_server::AppServer;
use crate::protocol::*;
use anyhow::{Context, Result, bail};
use std::io::{self, Write};

struct Selection {
    group: String,
    session: Option<String>,
    title: String,
}

pub(crate) fn run(app: &AppServer) -> Result<()> {
    let workspace = app.workspace()?;
    let group = app.default_group()?.id;
    let session = workspace
        .active_session_id
        .filter(|id| {
            workspace
                .sessions
                .iter()
                .any(|s| &s.id == id && s.group_id == group)
        })
        .or(app.sessions(Some(&group))?.first().map(|s| s.id.clone()));
    let mut selected = Selection {
        group,
        session,
        title: String::new(),
    };
    println!("xi CLI 已启动。输入 /help 查看命令，/exit 或 /quit 退出。\n");
    loop {
        print!("You: ");
        io::stdout().flush().context("无法刷新终端输出")?;
        let mut input = String::new();
        if io::stdin()
            .read_line(&mut input)
            .context("无法读取用户输入")?
            == 0
        {
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
            continue;
        }
        if let Err(error) = send(app, &mut selected, input) {
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
        .find(|p| p.active)
        .context("请先在应用端配置可用的模型服务")?;
    let model = provider
        .models
        .first()
        .filter(|_| provider.model.is_empty())
        .or_else(|| (!provider.model.is_empty()).then_some(&provider.model))
        .context("请先在应用端获取可用模型")?;
    if selected.session.is_none() {
        selected.session = Some(
            app.create_session(SessionCreateInput {
                group_id: selected.group.clone(),
                title: selected.title.clone(),
            })?
            .session_id,
        );
    }
    println!("\nAssistant:");
    app.chat_stream(
        ChatInput {
            content: content.to_owned(),
            session_id: selected.session.clone(),
            provider_id: Some(provider.id.clone()),
            model: Some(model.clone()),
            reasoning_effort: None,
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
        .context("当前没有会话，请先发送消息或用 /use 选择已有会话")
}

fn command(app: &AppServer, selected: &mut Selection, line: &str) -> Result<bool> {
    let (name, argument) = line
        .trim_start_matches('/')
        .split_once(char::is_whitespace)
        .unwrap_or((line.trim_start_matches('/'), ""));
    let argument = argument.trim();
    match name.to_ascii_lowercase().as_str() {
        "exit" | "quit" => return Ok(true),
        "help" => println!(
            "命令：\n/groups、/group-new <名称>、/group-use <ID或名称>、/group-rename <名称>\n/group-move <会话> <分组>、/group-archive [分组]、/group-delete [分组]\n/new [标题]、/sessions [--all]、/use <ID或标题>、/rename <标题>\n/clear、/archive、/search <关键词>、/export <路径>、/exit"
        ),
        "groups" => {
            for group in app.groups()? {
                println!(
                    "{}  {}（{} 个会话，更新时间 {}）",
                    group.id, group.name, group.conversation_count, group.updated_at
                );
            }
        }
        "group-new" => {
            let group = app.create_group(NamedInput {
                name: argument.to_owned(),
            })?;
            selected.group = group.id;
            app.select_group(&selected.group)?;
            selected.session = None;
            selected.title.clear();
            println!("已创建并切换到分组：{}", group.name);
        }
        "group-use" => {
            let group = app.resolve_group(argument)?;
            selected.session = app.sessions(Some(&group.id))?.first().map(|s| s.id.clone());
            selected.group = group.id;
            app.select_group(&selected.group)?;
            selected.title.clear();
            println!("已切换分组：{}", group.name);
        }
        "group-rename" => {
            app.rename_group(IdNamedInput {
                id: selected.group.clone(),
                name: argument.to_owned(),
            })?;
            println!("分组已重命名。");
        }
        "group-move" => {
            let (session, group) = argument
                .split_once(char::is_whitespace)
                .context("用法：/group-move <会话 ID 或名称> <分组 ID 或名称>")?;
            let session = app.resolve_session(session.trim())?;
            let group = app.resolve_group(group.trim())?;
            app.move_session(&session.id, &group.id)?;
            if selected.session.as_ref() == Some(&session.id) {
                selected.group = group.id;
            }
            println!("会话已移动到分组：{}", group.name);
        }
        "group-archive" | "group-delete" => {
            let id = if argument.is_empty() {
                selected.group.clone()
            } else {
                app.resolve_group(argument)?.id
            };
            if name.eq_ignore_ascii_case("group-archive") {
                app.archive_group(&id)?;
            } else {
                app.delete_group(&id)?;
            }
            if selected.group == id {
                let default = app
                    .groups()?
                    .into_iter()
                    .find(|g| g.is_system)
                    .context("未分组不存在")?;
                selected.group = default.id;
                app.select_group(&selected.group)?;
                selected.session = None;
                selected.title.clear();
            }
            println!("分组操作完成；删除分组时会话保留在未分组。");
        }
        "new" => {
            selected.session = None;
            selected.title = argument.to_owned();
            println!("已打开新会话草稿，发送首条消息后保存。");
        }
        "sessions" => {
            let group = if argument == "--all" {
                None
            } else {
                Some(selected.group.as_str())
            };
            for session in app.sessions(group)? {
                let group = app.resolve_group(&session.group_id)?;
                println!(
                    "{}  [{}] {}（模型：{}）",
                    session.id, group.name, session.title, session.model
                );
            }
        }
        "use" => {
            let session = app.resolve_session(argument)?;
            selected.group = session.group_id;
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
                session_id: Some(current(selected)?.to_owned()),
            })?;
            println!("会话消息已清空。");
        }
        "archive" | "delete" => {
            app.archive_session(current(selected)?)?;
            selected.session = app
                .sessions(Some(&selected.group))?
                .first()
                .map(|session| session.id.clone());
            if let Some(id) = &selected.session {
                app.select_session(id)?;
            }
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
            let output = app.export_session(current(selected)?)?;
            std::fs::write(argument, output)
                .with_context(|| format!("无法写入导出文件：{argument}"))?;
            println!("已导出当前会话。");
        }
        _ => bail!("未知命令：/{name}，输入 /help 查看帮助"),
    }
    Ok(false)
}
