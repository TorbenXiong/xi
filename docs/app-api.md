# 应用端 API

Runtime 绑定 127.0.0.1。桌面端使用端口 43127，通过 Tauri command runtime_token 获取凭据。业务请求必须包含 X-Xi-Token: <runtime-token>。Web 调试可通过客户端 URL 的 token 查询参数提供凭据，不要记录或分享完整 URL。

GET /?token=<runtime-token> 返回服务状态。OPTIONS 无需 Token，返回 204；实际业务仍鉴权。请求头上限 16 KiB，请求体上限 1 MiB，拒绝重复 Content-Length 和 Transfer-Encoding。错误响应为 { "error": "说明" }，不存在的路由返回 404。

HTTP 层负责传输、鉴权和 CORS，业务交由 AppServer；React 通过 XiClient 访问，CLI 直接调用同一业务服务。目前没有 HTTP 流式事件订阅。

## 模型服务

GET /api/providers 返回列表，每项含 id、name、base_url、api_key_configured、active 和 models，不返回密钥。active 用于首次加载与 CLI 默认项，窗口内切换由客户端维护。

models 是描述对象数组：

~~~json
{
  "id": "gpt-5.4",
  "reasoning_efforts": ["none", "low", "medium", "high", "xhigh"],
  "reasoning_source": "builtin"
}
~~~

来源为 builtin、provider、custom 或 unknown，策略见[模型能力](model-capabilities.md)。

POST /api/providers/models 获取模型，返回描述数组：

~~~json
{ "id": "provider-1", "base_url": "", "api_key": "" }
~~~

传 id 时，地址/Key 留空使用保存值；创建前预览可省略 id，传地址与 Key。服务请求 {base_url}/models，解析 data[].id 及可选能力扩展。只有实际凭据与保存值一致时更新缓存，移除下架型号，保留同模型的自定义能力；未保存凭据的预览不会污染缓存。

POST /api/providers 创建服务，返回 { "id": "provider-1" }：

~~~json
{
  "name": "",
  "base_url": "https://api.example.com/v1",
  "api_key": "YOUR_API_KEY",
  "models": [
    { "id": "example-model", "reasoning_efforts": [], "reasoning_source": "unknown" }
  ]
}
~~~

名称留空按域名生成。地址必须是有主机名的 HTTP/HTTPS URL，不得内嵌凭据、查询或片段。

- POST /api/providers/update：上述字段加 id；Key 留空保留原值，models 整体替换缓存，返回 { "saved": true }。
- POST /api/providers/delete：{ "id": "provider-1" }，至少保留一个服务，返回 { "deleted": true }。

## 项目

GET /api/workspace 返回 { projects, sessions, active_session_id }。项目为 { id, name, directory }；会话摘要为 { id, project_id, title, message_count }，project_id: null 为独立聊天。

POST /api/projects 创建或重新打开目录项目：

~~~json
{ "directory": "D:\\workspace\\my-project", "name": "" }
~~~

目录必须是现存的绝对文件夹，返回项目对象。name 可省略，默认使用文件夹名。同目录规范化后复用项目 ID，归档项目重开恢复；不同目录可同名。

- POST /api/projects/rename：{ "id": "project-1", "name": "新名称" }，返回更新项目。
- POST /api/projects/archive：{ "id": "project-1" }，隐藏项目及其会话，保留数据，返回 { "archived": true }。
- POST /api/projects/delete：{ "id": "project-1" }，移除关联，会话变为独立聊天，不删除本地文件，返回 { "deleted": true }。

Windows 桌面 command choose_project_directory 返回路径或取消值；Web 和非 Windows 桌面可手动输入。

## 会话

- POST /api/sessions：{ "project_id": "project-1", "title": "新会话" }，返回 { "session_id": "session-1" }。独立聊天传 project_id: null，title 可省略。
- POST /api/sessions/rename：{ "id": "session-1", "name": "新标题" }，返回 { "renamed": true }。
- POST /api/sessions/move：{ "id": "session-1", "project_id": null }，移动至项目或独立聊天，返回 { "moved": true }。
- POST /api/sessions/archive、POST /api/sessions/delete：传 { "id": "session-1" }。归档保留数据，删除永久移除会话、消息和运行记录。
- GET /api/sessions/history?session_id=session-1：返回含 sequence、role、content、status、error 的消息数组，包括空的失败消息和部分回复。
- GET /api/sessions/search?query=关键词：搜索活跃范围的标题和消息，返回 ID、项目 ID、标题、模型及更新时间。%、_ 和反斜杠按字面匹配。
- GET /api/sessions/export?session_id=session-1：返回 { "content": "Markdown 文本" }，包含状态和错误。
- POST /api/reset：必须传 { "session_id": "session-1" }，清空历史，保留会话，返回 { "reset": true }。

启动不自动创建空会话。React 的新会话先是本地草稿，首条消息才调用创建接口。默认选择只用于初始化和 CLI；读历史、发送、清空必须明确目标，未知 ID 报错。

## 聊天

POST /api/chat：

将请求体增加 `"stream": true` 时，Runtime 返回 `text/event-stream`。每个事件形如 `data: {"content":"增量文本"}`，以 `data: [DONE]` 结束；错误事件形如 `data: {"error":"..."}`。省略该字段仍返回完整 JSON。

~~~json
{
  "content": "你好",
  "session_id": "session-1",
  "provider_id": "provider-1",
  "model": "gpt-5.4",
  "reasoning_effort": "high"
}
~~~

前四项必填。reasoning_effort 可省略，表示服务默认；none 是实际值，不等于省略参数。先校验服务、缓存模型、推理选项和会话，再事务保存 user、assistant 占位及 run，然后请求模型。校验失败不写消息，网络失败保留输入和失败状态。

成功返回 { "session_id": "session-1", "message": { "role": "assistant", "content": "回复" } }。聊天、清空、删除、移动和归档通过同一锁协调，防止迟到回复破坏历史。

## 结构与边界

SQLite 表包括 projects、conversations、messages、runs、app_state、providers 和 provider_models。模型缓存保存能力数组和来源，服务配置无默认模型字段；模型由每次请求指定。

当前只初始化新结构，不迁移历史 schema、JSON 快照或 TOML。已删除旧分组 API、/api/messages、服务/会话 /select 路由及旧请求字段，DTO 拒绝未知字段。旧客户端必须更新，旧库应更换空目录。

API Key 当前明文保存在 SQLite。多个 UI 应连接同一 Runtime；窗口选择相互独立。远程连接、取消和事件订阅需要独立设计，当前本机 Token 和通配 CORS 不是完整远程认证方案。
