//! HTTP 是 App Server 的一个适配器；新增传输无需复制业务逻辑。
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, bail};
use serde::{Serialize, de::DeserializeOwned};

use crate::app_server::AppServer;
use crate::protocol::*;

const MAX_REQUEST_BODY: usize = 1024 * 1024;
const MAX_HEADER_BYTES: usize = 16 * 1024;
const MAX_CONNECTIONS: usize = 32;

struct HttpState {
    auth_token: String,
    app: Arc<AppServer>,
    connections: Mutex<usize>,
}

pub(crate) fn serve(app: Arc<AppServer>, port: Option<u16>, token: Option<String>) -> Result<()> {
    let state = Arc::new(HttpState {
        auth_token: token.unwrap_or_else(|| {
            format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            )
        }),
        app,
        connections: Mutex::new(0),
    });
    let listener =
        TcpListener::bind(("127.0.0.1", port.unwrap_or(0))).context("无法启动本地应用服务")?;
    println!(
        "xi Runtime 已启动：http://{}/?token={}",
        listener.local_addr()?,
        state.auth_token
    );
    println!("按 Ctrl+C 可退出 Runtime。");
    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                let Some(connection) = ConnectionGuard::acquire(Arc::clone(&state)) else {
                    let response = json_response(
                        "503 Service Unavailable",
                        &serde_json::json!({ "error": "连接数已达到上限" }),
                    )?;
                    let _ = write_response(&mut stream, response);
                    continue;
                };
                let state = Arc::clone(&state);
                std::thread::spawn(move || {
                    let _connection = connection;
                    if let Err(error) = handle_connection(stream, state) {
                        eprintln!("应用请求处理失败：{error:#}");
                    }
                });
            }
            Err(error) => eprintln!("接受应用请求失败：{error}"),
        }
    }
    Ok(())
}

struct ConnectionGuard(Arc<HttpState>);

impl ConnectionGuard {
    fn acquire(state: Arc<HttpState>) -> Option<Self> {
        {
            let mut count = state.connections.lock().ok()?;
            if *count >= MAX_CONNECTIONS {
                return None;
            }
            *count += 1;
        }
        Some(Self(state))
    }
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        if let Ok(mut count) = self.0.connections.lock() {
            *count = count.saturating_sub(1);
        }
    }
}

fn handle_connection(mut stream: TcpStream, state: Arc<HttpState>) -> Result<()> {
    stream.set_read_timeout(Some(std::time::Duration::from_secs(120)))?;
    stream.set_write_timeout(Some(std::time::Duration::from_secs(30)))?;
    let request = read_request(&mut stream)?;
    write_response(&mut stream, dispatch_request(request, &state))
}

struct HttpRequest {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

struct HttpResponse {
    status: &'static str,
    content_type: &'static str,
    body: Vec<u8>,
}

fn read_request(stream: &mut TcpStream) -> Result<HttpRequest> {
    let mut buffer = Vec::with_capacity(8192);
    let header_end;
    loop {
        let mut chunk = [0_u8; 4096];
        let bytes_read = stream.read(&mut chunk).context("读取应用请求失败")?;
        if bytes_read == 0 {
            bail!("请求提前结束");
        }
        buffer.extend_from_slice(&chunk[..bytes_read]);
        if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            header_end = position + 4;
            break;
        }
        if buffer.len() > MAX_HEADER_BYTES {
            bail!("请求头过大");
        }
    }
    let header_text = std::str::from_utf8(&buffer[..header_end]).context("请求头不是 UTF-8")?;
    let mut lines = header_text.split("\r\n");
    let request_line = lines.next().context("请求缺少首行")?;
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().context("请求缺少方法")?.to_owned();
    let path = request_parts.next().context("请求缺少路径")?.to_owned();
    let version = request_parts.next().context("请求缺少 HTTP 版本")?;
    if version != "HTTP/1.1" {
        bail!("只支持 HTTP/1.1");
    }
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect::<Vec<_>>();
    if headers.iter().any(|(name, _)| name == "transfer-encoding") {
        bail!("不支持 Transfer-Encoding");
    }
    let content_lengths = headers
        .iter()
        .filter(|(name, _)| name == "content-length")
        .count();
    if content_lengths > 1 {
        bail!("不支持重复 Content-Length");
    }
    let content_length = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .map(|(_, value)| value.parse::<usize>())
        .transpose()
        .context("Content-Length 格式错误")?
        .unwrap_or(0);
    if content_length > MAX_REQUEST_BODY {
        bail!("请求体过大");
    }
    while buffer.len() - header_end < content_length {
        let mut chunk = [0_u8; 4096];
        let bytes_read = stream.read(&mut chunk).context("读取请求体失败")?;
        if bytes_read == 0 {
            bail!("请求体提前结束");
        }
        buffer.extend_from_slice(&chunk[..bytes_read]);
    }
    Ok(HttpRequest {
        method,
        path,
        headers,
        body: buffer[header_end..header_end + content_length].to_vec(),
    })
}

fn query(path: &str, name: &str) -> Option<String> {
    let url = reqwest::Url::parse(&format!("http://localhost{path}")).ok()?;
    url.query_pairs()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
}

fn parse<T: DeserializeOwned>(body: &[u8]) -> Result<T> {
    serde_json::from_slice(body).context("请求格式错误")
}

fn ok<T: Serialize>(value: T) -> Result<HttpResponse> {
    json_response("200 OK", &value)
}

fn json_response<T: Serialize>(status: &'static str, value: &T) -> Result<HttpResponse> {
    Ok(HttpResponse {
        status,
        content_type: "application/json; charset=utf-8",
        body: serde_json::to_vec(value).context("无法序列化应用响应")?,
    })
}

fn write_response(stream: &mut TcpStream, response: HttpResponse) -> Result<()> {
    write!(
        stream,
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Headers: Content-Type, X-Xi-Token\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        response.status,
        response.content_type,
        response.body.len()
    )?;
    stream
        .write_all(&response.body)
        .context("写入应用响应失败")?;
    stream.flush().context("刷新应用响应失败")?;
    Ok(())
}

fn dispatch_request(request: HttpRequest, state: &HttpState) -> HttpResponse {
    if request.method == "OPTIONS" {
        return HttpResponse {
            status: "204 No Content",
            content_type: "text/plain; charset=utf-8",
            body: Vec::new(),
        };
    }
    let path = request.path.split('?').next().unwrap_or(&request.path);
    let authorized = if path == "/" {
        query(&request.path, "token").as_deref() == Some(&state.auth_token)
    } else {
        request
            .headers
            .iter()
            .any(|(key, value)| key == "x-xi-token" && value == &state.auth_token)
    };
    if !authorized {
        return json_response(
            "401 Unauthorized",
            &serde_json::json!({ "error": "未授权请求" }),
        )
        .expect("JSON 响应应可序列化");
    }
    let app = &state.app;
    let result = (|| -> Result<HttpResponse> {
        match (request.method.as_str(), path) {
            ("GET", "/") => {
                ok(serde_json::json!({ "service": "xi-agent-runtime", "status": "ok" }))
            }
            ("GET", "/api/providers") => ok(app.providers()?),
            ("GET", "/api/workspace") => ok(app.workspace()?),
            ("GET", "/api/messages") => {
                ok(app.messages(query(&request.path, "session_id").as_deref())?)
            }
            ("GET", "/api/sessions/history") => {
                ok(app.history(&query(&request.path, "session_id").context("需要 session_id")?)?)
            }
            ("GET", "/api/sessions/search") => {
                ok(app.search(&query(&request.path, "query").unwrap_or_default())?)
            }
            ("GET", "/api/sessions/export") => ok(
                serde_json::json!({ "content": app.export_session(&query(&request.path, "session_id").context("需要 session_id")?)? }),
            ),
            ("POST", "/api/providers") => {
                ok(serde_json::json!({ "id": app.create_provider(parse(&request.body)?)? }))
            }
            ("POST", "/api/providers/models") => ok(app.provider_models(parse(&request.body)?)?),
            ("POST", "/api/providers/update") => {
                app.update_provider(parse(&request.body)?)?;
                ok(serde_json::json!({ "saved": true }))
            }
            ("POST", "/api/providers/select") => {
                app.select_provider(&parse::<IdInput>(&request.body)?.id)?;
                ok(serde_json::json!({ "selected": true }))
            }
            ("POST", "/api/providers/delete") => {
                app.delete_provider(&parse::<IdInput>(&request.body)?.id)?;
                ok(serde_json::json!({ "deleted": true }))
            }
            ("POST", "/api/groups") => ok(app.create_group(parse(&request.body)?)?),
            ("POST", "/api/groups/rename") => ok(app.rename_group(parse(&request.body)?)?),
            ("POST", "/api/groups/delete") => {
                app.delete_group(&parse::<IdInput>(&request.body)?.id)?;
                ok(serde_json::json!({ "deleted": true }))
            }
            ("POST", "/api/groups/archive") => {
                app.archive_group(&parse::<IdInput>(&request.body)?.id)?;
                ok(serde_json::json!({ "archived": true }))
            }
            ("POST", "/api/sessions") => ok(app.create_session(parse(&request.body)?)?),
            ("POST", "/api/sessions/select") => {
                app.select_session(&parse::<IdInput>(&request.body)?.id)?;
                ok(serde_json::json!({ "selected": true }))
            }
            ("POST", "/api/sessions/rename") => ok(app.rename_session(parse(&request.body)?)?),
            ("POST", "/api/sessions/delete") => {
                app.delete_session(&parse::<IdInput>(&request.body)?.id)?;
                ok(serde_json::json!({ "deleted": true }))
            }
            ("POST", "/api/sessions/archive") => {
                app.archive_session(&parse::<IdInput>(&request.body)?.id)?;
                ok(serde_json::json!({ "archived": true }))
            }
            ("POST", "/api/sessions/move") => {
                let input = parse::<SessionMoveInput>(&request.body)?;
                app.move_session(&input.id, &input.group_id)?;
                ok(serde_json::json!({ "moved": true }))
            }
            ("POST", "/api/chat") => ok(app.chat(parse(&request.body)?)?),
            ("POST", "/api/reset") => {
                let target = if request.body.is_empty() {
                    SessionTarget::default()
                } else {
                    parse(&request.body)?
                };
                app.reset(target)?;
                ok(serde_json::json!({ "reset": true }))
            }
            _ => bail!("请求不存在"),
        }
    })();
    result.unwrap_or_else(|error| {
        let message = error.to_string();
        let status = if message == "请求不存在" {
            "404 Not Found"
        } else if path == "/api/chat"
            && !message.contains("配置中的")
            && !message.contains("消息不能为空")
            && !message.contains("不存在")
            && !message.contains("请求格式")
            && !message.contains("请先配置")
        {
            "502 Bad Gateway"
        } else if message.contains("状态不可用") || message.starts_with("无法") {
            "500 Internal Server Error"
        } else {
            "400 Bad Request"
        };
        json_response(status, &serde_json::json!({ "error": message }))
            .expect("JSON 响应应可序列化")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::new_id;
    use std::fs;
    use std::path::Path;

    fn with_app(test: impl FnOnce(Arc<AppServer>, &Path)) {
        let directory = std::env::temp_dir().join(new_id("xi-appserver-test"));
        fs::create_dir(&directory).unwrap();
        let path = directory.join("xi.sqlite3");
        let app = Arc::new(AppServer::open(&path).unwrap());
        test(app, &path);
        fs::remove_file(&path).unwrap();
        fs::remove_file(path.with_extension("lock")).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    fn request(method: &str, path: &str, body: serde_json::Value, auth: bool) -> HttpRequest {
        HttpRequest {
            method: method.to_owned(),
            path: path.to_owned(),
            headers: if auth {
                vec![("x-xi-token".to_owned(), "TEST_TOKEN".to_owned())]
            } else {
                Vec::new()
            },
            body: serde_json::to_vec(&body).unwrap(),
        }
    }

    #[test]
    fn workspace_bootstraps_protected_group_without_session() {
        with_app(|app, _| {
            let workspace = app.workspace().unwrap();
            assert_eq!(workspace.groups.len(), 1);
            assert!(workspace.groups[0].is_default);
            assert!(workspace.sessions.is_empty());
            assert!(workspace.active_session_id.is_none());
            let id = workspace.groups[0].id.clone();
            assert!(
                app.rename_group(IdNamedInput {
                    id: id.clone(),
                    name: "changed".to_owned()
                })
                .is_err()
            );
            assert!(app.delete_group(&id).is_err());
            assert_eq!(app.workspace().unwrap().groups[0].name, "未分组");
        });
    }

    #[test]
    fn http_preserves_auth_preflight_and_legacy_routes() {
        with_app(|app, _| {
            let state = HttpState {
                app,
                auth_token: "TEST_TOKEN".to_owned(),
                connections: Mutex::new(0),
            };
            let unauthorized = dispatch_request(
                request("GET", "/api/workspace", serde_json::Value::Null, false),
                &state,
            );
            assert_eq!(unauthorized.status, "401 Unauthorized");
            let preflight = dispatch_request(
                request("OPTIONS", "/api/chat", serde_json::Value::Null, false),
                &state,
            );
            assert_eq!(preflight.status, "204 No Content");
            let workspace = dispatch_request(
                request("GET", "/api/workspace", serde_json::Value::Null, true),
                &state,
            );
            let workspace: serde_json::Value = serde_json::from_slice(&workspace.body).unwrap();
            let created = dispatch_request(
                request(
                    "POST",
                    "/api/sessions",
                    serde_json::json!({
                        "group_id": workspace["groups"][0]["id"]
                    }),
                    true,
                ),
                &state,
            );
            let created: serde_json::Value = serde_json::from_slice(&created.body).unwrap();
            assert_eq!(created["created"], true);
            assert!(created["session_id"].as_str().is_some());
            let legacy = dispatch_request(
                request("GET", "/api/messages", serde_json::Value::Null, true),
                &state,
            );
            assert_eq!(legacy.status, "200 OK");
            assert_eq!(legacy.body, b"[]");
            let missing = dispatch_request(
                request(
                    "GET",
                    "/api/messages?session_id=missing%20session",
                    serde_json::Value::Null,
                    true,
                ),
                &state,
            );
            assert_eq!(missing.status, "400 Bad Request");
            assert_eq!(
                query("/?token=with%20space", "token").as_deref(),
                Some("with space")
            );
        });
    }

    fn provider(name: &str, base_url: &str, key: &str) -> ProviderInput {
        ProviderInput {
            id: None,
            name: name.to_owned(),
            base_url: base_url.to_owned(),
            api_key: key.to_owned(),
            model: String::new(),
            models: Vec::new(),
        }
    }

    #[test]
    fn streams_through_appserver_and_records_partial_failures() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let mock = std::thread::spawn(move || {
            for partial in [false, true] {
                let (mut stream, _) = listener.accept().unwrap();
                let request = read_request(&mut stream).unwrap();
                let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
                assert_eq!(body["stream"], true);
                let mut response =
                    "data: {\"choices\":[{\"delta\":{\"content\":\"你好\"}}]}\r\n\r\n"
                        .as_bytes()
                        .to_vec();
                if !partial {
                    response.extend_from_slice(b"data: [DONE]\n\n");
                }
                write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",response.len()).unwrap();
                for byte in response {
                    stream.write_all(&[byte]).unwrap();
                }
            }
        });
        with_app(|app, _| {
            let provider_id = app
                .create_provider(provider("stream", &base, "FIXTURE_ONLY"))
                .unwrap();
            let id = app
                .create_session(SessionCreateInput {
                    group_id: app.workspace().unwrap().groups[0].id.clone(),
                    title: String::new(),
                })
                .unwrap()
                .session_id;
            let input = |content: &str| ChatInput {
                content: content.to_owned(),
                session_id: Some(id.clone()),
                provider_id: Some(provider_id.clone()),
                model: Some("model".into()),
                ..Default::default()
            };
            let mut deltas = String::new();
            app.chat_stream(input("first"), |delta| {
                deltas.push_str(delta);
                Ok(())
            })
            .unwrap();
            assert_eq!(deltas, "你好");
            assert!(app.chat_stream(input("second"), |_| Ok(())).is_err());
            let history = app.history(&id).unwrap();
            assert_eq!(history[1].status, "complete");
            assert_eq!(history[3].status, "interrupted");
            assert_eq!(history[3].content, "你好");
            assert!(app.export_session(&id).unwrap().contains("interrupted"));
        });
        mock.join().unwrap();
    }

    #[test]
    fn explicit_targets_isolate_clients_and_restore_from_sqlite() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base_url = format!("http://{}/v1", listener.local_addr().unwrap());
        let mock = std::thread::spawn(move || {
            let mut captured = Vec::new();
            for _ in 0..3 {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                let request = read_request(&mut stream).unwrap();
                let response = if request.path == "/v1/models" {
                    json_response(
                        "200 OK",
                        &serde_json::json!({ "data": [{"id":"model-b"},{"id":"model-a"}] }),
                    )
                    .unwrap()
                } else {
                    let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
                    let fail =
                        body["messages"].as_array().unwrap().last().unwrap()["content"] == "fail";
                    let auth = request
                        .headers
                        .iter()
                        .find(|(k, _)| k == "authorization")
                        .unwrap()
                        .1
                        .clone();
                    captured.push((auth, body));
                    if fail {
                        json_response(
                            "503 Service Unavailable",
                            &serde_json::json!({ "error": "fixture failure" }),
                        )
                        .unwrap()
                    } else {
                        json_response("200 OK", &serde_json::json!({ "choices": [{ "message": { "role":"assistant", "content":"fixture answer" } }] })).unwrap()
                    }
                };
                write_response(&mut stream, response).unwrap();
            }
            captured
        });
        with_app(|app, path| {
            let first = app
                .create_provider(provider("first", &base_url, "TEST_A"))
                .unwrap();
            let second = app
                .create_provider(provider("second", &base_url, "TEST_B"))
                .unwrap();
            let models = app
                .provider_models(ProviderModelsInput {
                    id: Some(first.clone()),
                    ..Default::default()
                })
                .unwrap();
            assert_eq!(models, ["model-a", "model-b"]);
            app.select_provider(&second).unwrap();
            let group_id = app.workspace().unwrap().groups[0].id.clone();
            let session_a = app
                .create_session(SessionCreateInput {
                    group_id: group_id.clone(),
                    title: String::new(),
                })
                .unwrap()
                .session_id;
            let session_b = app
                .create_session(SessionCreateInput {
                    group_id,
                    title: String::new(),
                })
                .unwrap()
                .session_id;
            let answer = app
                .chat(ChatInput {
                    content: "hello".to_owned(),
                    session_id: Some(session_a.clone()),
                    provider_id: Some(first),
                    model: Some("model-a".to_owned()),
                    reasoning_effort: Some("high".to_owned()),
                })
                .unwrap();
            assert_eq!(answer.session_id, session_a);
            assert_eq!(app.messages(Some(&session_a)).unwrap().len(), 2);
            assert!(app.messages(Some(&session_b)).unwrap().is_empty());
            assert_eq!(
                app.workspace().unwrap().active_session_id.as_deref(),
                Some(session_b.as_str())
            );
            assert!(
                app.chat(ChatInput {
                    content: "fail".to_owned(),
                    session_id: Some(session_b.clone()),
                    provider_id: Some(second),
                    model: Some("model-b".to_owned()),
                    ..Default::default()
                })
                .is_err()
            );
            assert_eq!(app.messages(Some(&session_b)).unwrap().len(), 1);
            assert!(
                app.chat(ChatInput {
                    content: "invalid target".to_owned(),
                    session_id: Some(session_b.clone()),
                    provider_id: Some("missing".to_owned()),
                    model: Some("model-a".to_owned()),
                    ..Default::default()
                })
                .is_err()
            );
            app.reset(SessionTarget {
                session_id: Some(session_a.clone()),
            })
            .unwrap();
            assert!(app.messages(Some(&session_a)).unwrap().is_empty());
            assert_eq!(app.messages(Some(&session_b)).unwrap().len(), 1);
            drop(app);
            let reopened = AppServer::open(path).unwrap();
            assert_eq!(reopened.messages(Some(&session_b)).unwrap().len(), 1);
            assert_eq!(reopened.providers().unwrap()[0].models, models);
            let serialized = serde_json::to_string(&reopened.providers().unwrap()).unwrap();
            assert!(!serialized.contains("TEST_A") && !serialized.contains("TEST_B"));
        });
        let captured = mock.join().unwrap();
        assert_eq!(captured[0].0, "Bearer TEST_A");
        assert_eq!(captured[0].1["model"], "model-a");
        assert_eq!(captured[0].1["reasoning_effort"], "high");
        assert_eq!(captured[1].0, "Bearer TEST_B");
        assert_eq!(captured[1].1["messages"].as_array().unwrap().len(), 1);
    }
}
