# xi

xi 是一个以 Rust Runtime 为核心的多端 Agent 工作台原型。当前桌面端采用 Tauri 2 + React，通过可注入的 `XiClient` 访问统一的 `AppServer`，使用 SQLite 保存 Provider、分组、会话和消息。

## 当前能力

- Provider 管理：新增、编辑、删除、切换和模型列表获取。
- Provider 名称：留空时根据 API 域名自动生成，例如 `api.nassaapi.xyz` → `nassaapi`。
- 对话模型：配置页只获取并缓存模型；在输入框右下角切换模型和推理强度，Enter 发送、Shift + Enter 换行。
- 分组和会话：支持创建、切换、重命名、移动、归档、搜索和导出；删除普通分组会将历史移到“未分组”，删除会话需要确认。新会话发送首条消息后才加入列表。
- 持久化：沿用主线的关系化 SQLite 数据库，保存消息和运行状态；Provider 与密钥由页面管理，不再读取 TOML 配置。
- Runtime：本机 `127.0.0.1` HTTP API，使用随机 Token 鉴权，供 Tauri、Web 或未来移动端复用。

## 启动桌面端

在已经安装锁定依赖、Rust 和 Tauri 平台构建环境的机器上，从仓库根目录执行：

```powershell
cargo build --offline --locked
& .\app\node_modules\.bin\tauri.cmd dev --config .\src-tauri\tauri.conf.json
```

Tauri CLI 会启动 Vite 开发服务和桌面窗口；桌面壳再启动 `xi-agent-cli --app-server` 子进程。应用窗口打开后，在“模型配置”中填写 API 地址和 API Key，点击“获取模型”，保存 Provider；进入对话后从输入框右下角选择模型和推理强度。

开发服务不可用时，直接运行旧的桌面二进制可能显示连接失败，或显示上次构建时嵌入的页面。开发期间使用上面的 Tauri CLI 命令，以保证页面与 Runtime 版本一致。

## 启动 Runtime 或 CLI

只启动 Runtime：

```powershell
cargo run -- --app-server
```

CLI 调试入口：

```powershell
cargo run -- --cli
```

CLI 也调用 `AppServer`，使用默认 Provider 的已保存模型；模型为空时使用缓存列表第一项。首条输入才创建会话，消息会持久化到 SQLite。裸 `cargo run` 只启动随机端口的 HTTP 服务，根路径返回状态 JSON。

CLI 保留 `/groups`、`/group-new`、`/group-use`、`/group-rename`、`/group-move`、`/group-archive`、`/group-delete`、`/new`、`/sessions`、`/use`、`/rename`、`/clear`、`/archive`（别名 `/delete`）、`/search` 和 `/export`；`/help` 查看用法。CLI 使用流式输出，普通 JSON 响应也可回退处理。两种调用都记录 `complete`、`failed` 或 `interrupted` 状态，中断文本可通过页面或导出查看。

## 数据位置与迁移

默认数据库沿用主线位置：Windows 为 `%LOCALAPPDATA%\xi\xi.db`，macOS 为 `~/Library/Application Support/xi/xi.db`，Linux 为 `$XDG_DATA_HOME/xi/xi.db`（未设置时使用 `~/.local/share/xi/xi.db`）。设置进程环境变量 `XI_DATA_DIR` 后改用该目录下的 `xi.db`，用于隔离测试数据。

启动时将工作目录下 `config/xi.sqlite3` 中的客户端快照和 Provider 配置只读导入统一数据库；设置 `XI_DATA_DIR` 后只检查该测试目录下的 `xi.sqlite3`。导入使用事务，成功后标记，重复启动不会重复导入或覆盖既有主线历史，来源文件保留。直接打开含旧快照的数据库也会将快照迁入关系表。

同一数据库只允许一个 Runtime 实例，防止误恢复另一个活动进程的请求；目录中的 `xi.lock` 是实例锁，退出后文件可以保留。多个 UI 应连接同一 Runtime。Token 通过 Tauri command 或 `XI_RUNTIME_TOKEN` 注入，不能把带 Token 的调试 URL 提交到仓库。

## 目录

- `src/main.rs` / `src/cli.rs`：进程装配 / CLI 命令与流式显示。
- `src/protocol.rs`：界面无关的请求、响应类型。
- `src/app_server.rs`：Provider、分组、会话和聊天业务入口。
- `src/http.rs`：HTTP 解析、Token 鉴权、CORS 和路由映射。
- `src/model.rs` / `src/storage.rs`：模型服务适配 / SQLite 读写。
- `src/storage/providers.rs` / `src/storage/migration.rs`：Provider 读写 / 旧客户端数据库导入。
- `app/src/client/`：`XiClient` 接口和 HTTP 适配；组件不直接调用底层接口。
- `app/src/platform/`：Tauri/Web 的 Runtime 凭据获取。
- `src-tauri/`：Tauri 2 桌面壳和 Runtime 子进程管理。
- [客户端图标](app/assets/xi-icon-prompt.md)：生成提示词、原图位置及图标转换方法。
- [应用端 API](docs/app-api.md)：请求、鉴权与存储约定。
- 学习路径：[00 技术选型](docs/learning/00-技术选型.md) → [01 单次对话](docs/learning/01-单次对话.md) → [02 循环对话](docs/learning/02-循环对话.md) → [03 会话持久化与分组](docs/learning/03-会话持久化与分组.md) → [04 应用端架构与调试](docs/learning/04-应用端架构与调试.md)。
- [持久化方案](docs/方案-会话持久化与分组.md)：数据边界和后续上下文治理。

## 验证

```powershell
cargo fmt --check
cargo test --offline --locked
cargo clippy --offline --locked --all-targets --all-features -- -D warnings
npm --prefix app run build
cargo check --offline --locked --manifest-path src-tauri/Cargo.toml
```

本机 Node.js 24 可直接运行无外部服务的客户端测试：

```powershell
node --experimental-strip-types --test app/tests/client.test.mjs app/tests/runtime.test.mjs
```

`runtime.test.mjs` 需先执行 `cargo build --offline --locked`，使用临时数据库和本机 mock 模型服务，验证 HTTP 调用、CLI 命令、流式输出、中断状态和重启恢复，不访问真实配置。

运行数据、SQLite 临时文件、本地依赖、构建产物和 Tauri 自动生成的 schema 已加入 `.gitignore`。API Key 只通过配置页写入本地数据库，不应出现在提交内容中。

## 当前边界

当前保留 HTTP JSON API，桌面聊天等待完整结果，CLI 支持流式输出；两者均由 AppServer 串行协调。尚未接入 JSON-RPC、执行事件订阅、Turn 调度或取消机制。归档历史保留，但界面暂未提供取消归档；上下文裁剪、Web/移动端实机及 Runtime sidecar 正式分发仍待完善。API Key 当前明文保存在本地 SQLite。
