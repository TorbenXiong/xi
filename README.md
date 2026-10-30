# xi

xi 是以 Rust Runtime 为核心的 Agent 工作台原型。桌面端采用 Tauri 2 + React，通过可注入的 XiClient 访问统一的 AppServer，使用 SQLite 保存项目、会话、消息和模型服务配置。

## 当前能力

- 项目关联真实本地文件夹，按目录组织会话。Windows 支持系统文件夹选择器，也可手动填写绝对路径。同目录重复打开复用项目，不同目录可同名。
- 输入框上方的项目选择器支持搜索项目、创建项目和切换到“不在项目中工作”；已有会话切换项目会移动会话，草稿会保留在当前窗口。
- 独立会话显示在“聊天”区域。移除项目只解除关联，会话保留为独立聊天，不删除本地文件；归档隐藏项目及其会话。
- 聊天输入区依次选择服务、模型和推理强度。切换服务或模型后恢复默认推理，档位来自所选模型能力，未知型号使用服务默认。
- 设置页提供常规、外观、模型服务和快捷键分类。模型服务支持获取模型、逐模型自定义推理选项，点击“保存配置”生效。
- 会话支持重命名、移动、归档、搜索、Markdown 导出、清空和删除。新会话发送首条消息后加入列表。
- 草稿按会话保留在当前窗口内存，发送失败恢复输入；历史加载失败时提供重试并暂时禁止发送。
- 用户消息、运行状态和失败/中断的部分回复持久化。CLI 支持模型流式输出。
- 本机 HTTP API 绑定 127.0.0.1，使用随机 Token 鉴权，供桌面/Web 客户端复用。

## 启动桌面端

以下命令要求本机已有 Rust、Tauri 平台构建环境及仓库锁定的前端依赖，不会安装依赖。从仓库根目录执行：

~~~powershell
$env:CARGO_NET_OFFLINE = 'true'
cargo build --offline --locked
& .\app\node_modules\.bin\tauri.cmd dev --config .\src-tauri\tauri.conf.json --no-watch
~~~

Tauri CLI 启动 Vite 与桌面窗口，桌面壳启动 xi-agent-cli --app-server 子进程。窗口默认居中。进入“设置 → 模型服务”，填写 API 地址和 Key，获取模型并保存，然后在聊天输入区选择服务、模型和推理选项。

开发期应使用上述命令以保证页面和 Runtime 版本一致。直接运行旧桌面二进制可能加载上次构建的页面，或因 Vite 未启动而连接失败。

## Runtime 与 CLI

~~~powershell
cargo run --offline --locked -- --app-server
cargo run --offline --locked -- --cli
~~~

CLI 与桌面端共用数据库和 AppServer。输入 /help 查看参数。当前命令包括：

- 项目：/projects、/project-open <绝对目录>、/project-use <ID/名称/->、/project-rename、/project-move、/project-archive、/project-remove；/project-use - 选择独立聊天。
- 模型：/providers、/provider-use、/models、/model、/reasoning。
- 会话：/new、/sessions [--all]、/use、/rename、/clear、/archive、/search、/export、/exit。

## 数据位置

默认数据库：Windows 为 %LOCALAPPDATA%\xi\xi.db，macOS 为 ~/Library/Application Support/xi/xi.db，Linux 为 $XDG_DATA_HOME/xi/xi.db（未设置时为 ~/.local/share/xi/xi.db）。进程环境变量 XI_DATA_DIR 可指定独立目录，使用其中的 xi.db，适合开发和测试隔离。

当前项目未上线，直接初始化当前结构，不迁移旧分组、JSON 快照、TOML 配置或旧客户端数据库。旧结构不能直接复用，应使用空数据目录；程序不会自动删除用户文件。

同一数据库只允许一个 Runtime，多个 UI 应连接同一服务。xi.lock 使用操作系统文件锁，退出后文件存在不代表仍被占用。API Key 当前明文保存在 SQLite，查询 API 只返回是否配置；不要提交数据库、真实密钥或带 Token 的 URL。

项目目录目前用于组织会话，不会自动读取文件、注入文件内容或赋予模型文件执行能力。

## 代码与文档

- src/protocol.rs：协议；src/app_server.rs：业务；src/http.rs：HTTP、鉴权与 CORS。
- src/model.rs：Chat Completions、SSE 与能力；src/storage.rs、src/storage/providers.rs：SQLite。
- src/cli.rs：CLI；src-tauri/：窗口、Runtime 子进程与 Windows 文件夹选择。
- app/src/client/：类型、XiClient 与 HTTP 适配；app/src/platform/：平台能力；app/src/preferences.ts：当前界面偏好。
- [应用 API](docs/app-api.md)、[模型能力](docs/model-capabilities.md)、[项目与会话方案](docs/方案-项目与会话.md)、[审查记录](docs/review-2026-10-29.md)。
- 学习路径：[00 技术选型](docs/learning/00-技术选型.md) → [01 单次对话](docs/learning/01-单次对话.md) → [02 循环对话](docs/learning/02-循环对话.md) → [03 项目会话与持久化](docs/learning/03-项目会话与持久化.md) → [04 应用端架构与调试](docs/learning/04-应用端架构与调试.md)。
- [客户端图标](app/assets/xi-icon-prompt.md)：生成提示词与转换方法。

## 验证

~~~powershell
cargo fmt --check
cargo test --offline --locked
cargo clippy --offline --locked --all-targets --all-features -- -D warnings
cargo build --offline --locked
npm --prefix app run build
cargo check --offline --locked --manifest-path src-tauri/Cargo.toml
node --experimental-strip-types --test app/tests/client.test.mjs app/tests/preferences.test.mjs app/tests/models.test.mjs app/tests/runtime.test.mjs
~~~

Node 测试使用本机 Node.js 24。Runtime 进程测试需先构建根项目，使用临时库与本机 mock 模型，不读取真实凭据、不访问真实模型服务。

## 当前边界

桌面 HTTP 等待完整回复，CLI 支持流式输出。尚未实现工具执行、上下文预算、JSON-RPC、客户端事件订阅、Turn 调度和取消。仅支持 Chat Completions 形态，未实现 Anthropic 原生 Messages 或 OpenAI Responses。非 Windows 文件夹选择暂需手动输入。Web/移动端实机、归档恢复界面及正式 Runtime sidecar 分发仍待完善。
