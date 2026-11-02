#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use rand::random;
use std::path::PathBuf;
use tauri::{Manager, RunEvent, State};

mod project_directory;
mod runtime;
use runtime::RuntimeProcess;

struct RuntimeToken(String);

#[tauri::command]
fn runtime_token(state: State<'_, RuntimeToken>) -> String {
    state.0.clone()
}

#[tauri::command]
fn runtime_status(state: State<'_, RuntimeProcess>) -> Result<runtime::Snapshot, String> {
    state.snapshot()
}

#[tauri::command]
fn retry_runtime(state: State<'_, RuntimeProcess>) -> Result<(), String> {
    state.retry()
}

fn generate_token() -> String {
    let bytes: [u8; 32] = random();
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn runtime_binary() -> PathBuf {
    let name = format!("xi-agent-cli{}", std::env::consts::EXE_SUFFIX);
    if cfg!(debug_assertions) {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target/debug")
            .join(name)
    } else {
        std::env::current_exe()
            .unwrap_or_else(|_| PathBuf::from(&name))
            .with_file_name(name)
    }
}

fn runtime_working_directory() -> PathBuf {
    if cfg!(debug_assertions) {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
    } else {
        std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("."))
    }
}

fn main() {
    let token = generate_token();
    let process = RuntimeProcess::start(runtime_binary(), runtime_working_directory(), token.clone());

    let app = tauri::Builder::default()
        .manage(RuntimeToken(token))
        .manage(process)
        .invoke_handler(tauri::generate_handler![
            runtime_token,
            runtime_status,
            retry_runtime,
            project_directory::choose_project_directory
        ])
        .build(tauri::generate_context!())
        .expect("error while building xi desktop");
    app.run(handle_run_event);
}

fn handle_run_event(app: &tauri::AppHandle, event: RunEvent) {
    if matches!(event, RunEvent::Exit) {
        if let Some(process) = app.try_state::<RuntimeProcess>() {
            process.stop();
        }
    }
}
