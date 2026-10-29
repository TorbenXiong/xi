#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use rand::random;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::Mutex;
use tauri::{Manager, RunEvent, State};

struct RuntimeToken(String);
struct RuntimeProcess(Mutex<Option<Child>>);

#[tauri::command]
fn runtime_token(state: State<'_, RuntimeToken>) -> String {
    state.0.clone()
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
    let mut command = Command::new(runtime_binary());
    command
        .arg("--app-server")
        .current_dir(runtime_working_directory())
        .env("XI_RUNTIME_TOKEN", &token);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW：只隐藏 Runtime 控制台。
    }
    let child = command.spawn().expect("无法启动 xi Agent Runtime");

    let app = tauri::Builder::default()
        .manage(RuntimeToken(token))
        .manage(RuntimeProcess(Mutex::new(Some(child))))
        .invoke_handler(tauri::generate_handler![runtime_token])
        .build(tauri::generate_context!())
        .expect("error while building xi desktop");
    app.run(handle_run_event);
}

fn stop_runtime(process: &RuntimeProcess) {
    if let Ok(mut child) = process.0.lock() {
        if let Some(mut child) = child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for RuntimeProcess {
    fn drop(&mut self) {
        stop_runtime(self);
    }
}

fn handle_run_event(app: &tauri::AppHandle, event: RunEvent) {
    if matches!(event, RunEvent::Exit) {
        if let Some(process) = app.try_state::<RuntimeProcess>() {
            stop_runtime(&process);
        }
    }
}
