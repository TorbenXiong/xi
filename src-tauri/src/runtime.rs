//! 只有监督线程拥有子进程；停止、重试与健康检查不会并发创建 Runtime。
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const PROBE_INTERVAL: Duration = Duration::from_secs(2);
const PROBE_TIMEOUT: Duration = Duration::from_secs(1);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const STABLE_PERIOD: Duration = Duration::from_secs(30);
const MAX_RESTARTS: u32 = 5;

// 使用基础类型返回 Tauri IPC，避免为进程管理增加依赖。
pub type Snapshot = (String, Option<u32>, u32, Option<String>);

pub struct RuntimeProcess {
    status: Arc<Mutex<Snapshot>>,
    sender: mpsc::Sender<Action>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

enum Action {
    Retry,
    Stop,
}

struct Config {
    binary: PathBuf,
    directory: PathBuf,
    token: String,
    address: SocketAddr,
}

impl RuntimeProcess {
    pub fn start(binary: PathBuf, directory: PathBuf, token: String) -> Self {
        let (sender, receiver) = mpsc::channel();
        let status = Arc::new(Mutex::new(("starting".into(), None, 0, None)));
        let shared = Arc::clone(&status);
        let worker = thread::spawn(move || {
            supervise(
                Config {
                    binary,
                    directory,
                    token,
                    address: SocketAddr::from(([127, 0, 0, 1], 43127)),
                },
                receiver,
                shared,
            );
        });
        Self {
            status,
            sender,
            worker: Mutex::new(Some(worker)),
        }
    }

    pub fn snapshot(&self) -> Result<Snapshot, String> {
        self.status.lock().map(|status| status.clone()).map_err(|_| "Runtime 状态不可用".into())
    }

    pub fn retry(&self) -> Result<(), String> {
        self.sender.send(Action::Retry).map_err(|_| "Runtime 监督线程已停止".into())
    }

    pub fn stop(&self) {
        // 持有 worker 锁直到 join 结束，确保退出回调和 Drop 都等待子进程回收。
        if let Ok(mut worker) = self.worker.lock() {
            if let Some(worker) = worker.take() {
                let _ = self.sender.send(Action::Stop);
                let _ = worker.join();
            }
        }
    }
}

impl Drop for RuntimeProcess {
    fn drop(&mut self) {
        self.stop();
    }
}

fn spawn(config: &Config) -> std::io::Result<Child> {
    let mut command = Command::new(&config.binary);
    command.arg("--app-server").current_dir(&config.directory)
        .env("XI_RUNTIME_TOKEN", &config.token)
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(test)]
    if config.binary == std::env::current_exe().unwrap() {
        // 子进程运行隔离的 HTTP fixture，不占用桌面固定端口或真实数据库。
        command = Command::new(&config.binary);
        command.args(["--ignored", "--exact", "runtime::tests::fixture", "--nocapture"])
            .current_dir(&config.directory)
            .env("XI_RUNTIME_TOKEN", &config.token)
            .env("XI_RUNTIME_TEST_ADDRESS", config.address.to_string())
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    command.spawn()
}

fn stop_child(child: &mut Option<Child>) {
    if let Some(mut process) = child.take() {
        let _ = process.kill();
        let _ = process.wait();
    }
}

fn restart_delay(restarts: &mut u32) -> Option<Duration> {
    if *restarts >= MAX_RESTARTS { return None; }
    let delay = Duration::from_secs(1 << *restarts);
    *restarts += 1;
    Some(delay)
}

fn probe(address: SocketAddr, token: &str) -> Result<(), String> {
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let mut stream = TcpStream::connect_timeout(&address, PROBE_TIMEOUT)
        .map_err(|_| "无法连接 Runtime".to_string())?;
    stream.set_write_timeout(Some(PROBE_TIMEOUT)).map_err(|_| "无法设置健康检查超时")?;
    // Token 只放在请求头，避免进入 URL 或日志。
    write!(stream, "GET / HTTP/1.1\r\nHost: {address}\r\nX-Xi-Token: {token}\r\nConnection: close\r\n\r\n")
        .map_err(|_| "无法发送健康检查")?;
    let mut response = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() { return Err("Runtime 健康检查超时".into()); }
        stream.set_read_timeout(Some(remaining)).map_err(|_| "无法设置健康检查超时")?;
        let mut buffer = [0; 1024];
        let count = stream.read(&mut buffer).map_err(|_| "Runtime 健康检查超时或连接中断")?;
        if count == 0 { break; }
        response.extend_from_slice(&buffer[..count]);
        if response.len() > 8192 { return Err("Runtime 健康响应过大".into()); }
    }
    let response = std::str::from_utf8(&response).map_err(|_| "Runtime 健康响应无效")?;
    let (headers, body) = response.split_once("\r\n\r\n").ok_or("Runtime 健康响应无效")?;
    if headers.starts_with("HTTP/1.1 401 ") { return Err("Runtime 凭据不匹配，请检查端口占用".into()); }
    if !headers.starts_with("HTTP/1.1 200 ") || body != r#"{"service":"xi-agent-runtime","status":"ok"}"# {
        return Err("Runtime 健康响应无效，请检查端口占用".into());
    }
    Ok(())
}

fn supervise(config: Config, receiver: mpsc::Receiver<Action>, status: Arc<Mutex<Snapshot>>) {
    let mut child: Option<Child> = None;
    let mut generation = 0;
    let mut restarts = 0;
    let mut next_start = Some(Instant::now());
    let mut started = Instant::now();
    let mut next_probe = Instant::now();
    let mut ready_since = None;
    let mut failures = 0;
    let mut last_error = None;
    loop {
        match receiver.recv_timeout(Duration::from_millis(200)) {
            Ok(Action::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Ok(Action::Retry) => {
                // 已在启动或恢复时忽略重复点击，避免重启正在工作的进程。
                if child.is_none() && next_start.is_none() {
                    restarts = 0;
                    last_error = None;
                    next_start = Some(Instant::now());
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        let now = Instant::now();
        let mut failure = None;
        if child.is_none() && next_start.is_some_and(|time| now >= time) {
            next_start = None;
            match spawn(&config) {
                Ok(process) => {
                    generation += 1;
                    child = Some(process);
                    started = now;
                    next_probe = now;
                    ready_since = None;
                    failures = 0;
                }
                Err(error) => failure = Some(format!("无法启动 Runtime：{error}")),
            }
        }
        if let Some(process) = child.as_mut() {
            match process.try_wait() {
                Ok(Some(exit)) => failure = Some(format!("Runtime 已退出（{exit}），请检查端口或数据库是否被占用")),
                Err(error) => failure = Some(format!("无法查询 Runtime 进程：{error}")),
                Ok(None) if now >= next_probe => {
                    next_probe = now + PROBE_INTERVAL;
                    match probe(config.address, &config.token) {
                        Ok(()) => {
                            failures = 0;
                            last_error = None;
                            let since = ready_since.get_or_insert(now);
                            if now.duration_since(*since) >= STABLE_PERIOD { restarts = 0; }
                        }
                        Err(error) => {
                            failures += 1;
                            // 启动有宽限期；就绪后连续失败三次才回收子进程。
                            if (ready_since.is_some() && failures >= 3)
                                || (ready_since.is_none() && now.duration_since(started) >= STARTUP_TIMEOUT) {
                                failure = Some(error);
                            }
                        }
                    }
                }
                Ok(None) => {}
            }
        }
        if let Some(error) = failure {
            stop_child(&mut child);
            ready_since = None;
            last_error = Some(error);
            next_start = restart_delay(&mut restarts).map(|delay| Instant::now() + delay);
        }
        let phase = if ready_since.is_some() { "ready" }
            else if child.is_none() && next_start.is_none() { "failed" }
            else if generation <= 1 && restarts == 0 { "starting" }
            else { "recovering" };
        if let Ok(mut status) = status.lock() {
            *status = (phase.into(), child.as_ref().map(Child::id), generation, last_error.clone());
        }
    }
    stop_child(&mut child);
    if let Ok(mut status) = status.lock() { *status = ("stopped".into(), None, generation, None); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::net::TcpListener;

    #[test]
    fn restart_budget_uses_bounded_exponential_backoff() {
        let mut attempts = 0;
        for seconds in [1, 2, 4, 8, 16] {
            assert_eq!(restart_delay(&mut attempts), Some(Duration::from_secs(seconds)));
        }
        assert_eq!(restart_delay(&mut attempts), None);
        attempts = 0; // 手动重试或稳定运行后获得新的预算。
        assert_eq!(restart_delay(&mut attempts), Some(Duration::from_secs(1)));
    }

    #[test]
    #[ignore = "仅由监督器测试作为子进程启动"]
    fn fixture() {
        let address = std::env::var("XI_RUNTIME_TEST_ADDRESS").unwrap();
        let listener = TcpListener::bind(address).unwrap();
        listener.set_nonblocking(true).unwrap();
        loop {
            if fs::remove_file("crash-once").is_ok() { std::process::exit(17); }
            if let Ok((mut stream, _)) = listener.accept() {
                stream.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
                let mut request = [0; 2048];
                let count = stream.read(&mut request).unwrap_or(0);
                let request = String::from_utf8_lossy(&request[..count]);
                let (status, body) = if !request.contains("X-Xi-Token: TEST_TOKEN\r\n") {
                    ("401 Unauthorized", "{}")
                } else if std::path::Path::new("unhealthy").exists() {
                    ("503 Service Unavailable", "{}")
                } else {
                    ("200 OK", r#"{"service":"xi-agent-runtime","status":"ok"}"#)
                };
                let _ = write!(stream, "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn wait_for(process: &RuntimeProcess, predicate: impl Fn(&Snapshot) -> bool) -> Snapshot {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let status = process.snapshot().unwrap();
            if predicate(&status) { return status; }
            assert!(Instant::now() < deadline, "Runtime 状态等待超时：{status:?}");
            thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn supervisor_recovers_crashes_and_unhealthy_children_and_reaps_on_stop() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let directory = std::env::temp_dir().join(format!("xi-supervisor-{}-{}", std::process::id(), address.port()));
        fs::create_dir(&directory).unwrap();
        let (sender, receiver) = mpsc::channel();
        let status = Arc::new(Mutex::new(("starting".into(), None, 0, None)));
        let shared = Arc::clone(&status);
        let config = Config { binary: std::env::current_exe().unwrap(), directory: directory.clone(),
            token: "TEST_TOKEN".into(), address };
        let process = RuntimeProcess { status, sender,
            worker: Mutex::new(Some(thread::spawn(move || supervise(config, receiver, shared)))) };
        let first = wait_for(&process, |s| s.0 == "ready");
        assert!(probe(address, "WRONG_TOKEN").unwrap_err().contains("凭据"));
        process.retry().unwrap(); // 健康进程不能被重试按钮重启。
        thread::sleep(Duration::from_millis(400));
        assert_eq!(process.snapshot().unwrap().1, first.1);
        fs::write(directory.join("crash-once"), b"").unwrap();
        let second = wait_for(&process, |s| s.0 == "ready" && s.2 > first.2);
        assert_ne!(first.1, second.1);
        fs::write(directory.join("unhealthy"), b"").unwrap();
        // 第一轮失败不回收进程，连续失败达到阈值才恢复。
        thread::sleep(PROBE_INTERVAL + Duration::from_millis(300));
        assert_eq!(process.snapshot().unwrap().1, second.1);
        wait_for(&process, |s| s.0 == "recovering" && s.1.is_none());
        fs::remove_file(directory.join("unhealthy")).unwrap();
        wait_for(&process, |s| s.0 == "ready" && s.2 > second.2);
        process.stop();
        assert_eq!(process.snapshot().unwrap().0, "stopped");
        assert!(TcpListener::bind(address).is_ok());
        drop(process);
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn missing_binary_reports_failure_and_stop_cancels_pending_restart() {
        let process = RuntimeProcess::start(PathBuf::from("xi-missing-fixture-binary"),
            std::env::temp_dir(), "TEST_TOKEN".into());
        let status = wait_for(&process, |s| s.3.is_some());
        assert_eq!(status.0, "recovering");
        assert!(status.1.is_none());
        process.stop();
        thread::sleep(Duration::from_millis(300));
        assert_eq!(process.snapshot().unwrap().0, "stopped");
    }
}
