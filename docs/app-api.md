# 应用端 API

Runtime 绑定 `127.0.0.1`，桌面模式使用端口 `43127`。`/api/*` 业务请求需要携带：

```http
X-Xi-Token: <runtime-token>
```

桌面端通过 Tauri command `runtime_token` 获取 Token；Web 调试可通过前端 URL 的 `token` 查询参数传入。`GET /?token=<runtime-token>` 返回状态 JSON；无 Token 时也返回 401。`OPTIONS` 预检无需 Token，返回 204 和 CORS 头，业务请求仍须鉴权。

HTTP 层仅将请求映射到 `AppServer` 的类型化方法；鉴权和 CORS 不进入业务服务。React 通过注入的 `XiClient` 调用这些能力，CLI 在进程内调用同一个 `AppServer`。本轮保留所有既有路径，没有实现 JSON-RPC 或事件订阅。

## 多客户端选择

`Provider.active` 和 `Workspace.active_session_id` 是服务端保存的默认项，供首次加载和旧客户端回退使用。当前 React 窗口自行维护所选 Provider、会话和模型，普通界面切换不会调用 `/select` 修改其他窗口的选择。

读取消息、发送消息和清空历史时应显式传入目标 ID。未知 ID 返回错误，不回退到默认项。旧客户端省略 ID 时仍使用服务端默认项。

## Provider

`GET /api/providers` 返回 Provider 列表，不返回 API Key，只返回 `api_key_configured`。每个 Provider 的 `models` 是已缓存的模型 ID。

`POST /api/providers/models` 获取模型列表：

```json
{
  "id": "provider-1",
  "base_url": "https://api.example.com/v1",
  "api_key": "YOUR_API_KEY"
}
```

传入 `id` 时，`base_url` 和 `api_key` 可以留空，Runtime 会从 SQLite 读取已保存值。服务端请求 `{base_url}/models`，解析 OpenAI-compatible 的 `data[].id`，返回字符串数组；带 `id` 的请求还会向 `provider_models` 追加缓存。未保存的新配置先把列表留在表单中，保存 Provider 时再写入数据库；取消弹窗不保存新配置。

`POST /api/providers` 创建 Provider。`name` 可以为空，Runtime 会从 API 域名生成名称；`model` 可以为空，模型在对话中选择。

```json
{
  "name": "",
  "base_url": "https://api.example.com/v1",
  "api_key": "YOUR_API_KEY",
  "model": "",
  "models": ["model-a", "model-b"]
}
```

其他 Provider 操作：

- `POST /api/providers/update`：更新配置，`id` 必填，API Key 为空时保留原值。
- `POST /api/providers/select`：`{ "id": "provider-1" }`。
- `POST /api/providers/delete`：`{ "id": "provider-1" }`，至少保留一个 Provider。

## 工作区和会话

- `GET /api/workspace`：返回分组、会话摘要和 `active_session_id`。
- `GET /api/messages?session_id=session-1`：返回指定会话消息；省略参数时使用默认会话。
- `POST /api/groups`：`{ "name": "项目" }`。
- `POST /api/groups/rename`：`{ "id": "group-1", "name": "新名称" }`。
- `POST /api/groups/delete`：隐藏普通分组，将其会话移到“未分组”并保留消息；默认“未分组”不能删除。
- `POST /api/groups/archive`：`{ "id": "group-1" }`，归档分组；系统分组不可归档。
- `POST /api/sessions`：`{ "group_id": "group-1", "title": "新会话" }`。
- `POST /api/sessions/select`：`{ "id": "session-1" }`。
- `POST /api/sessions/rename`：`{ "id": "session-1", "name": "新标题" }`。
- `POST /api/sessions/delete`：`{ "id": "session-1" }`。
- `POST /api/sessions/archive`：`{ "id": "session-1" }`，隐藏会话但保留历史。
- `POST /api/sessions/move`：`{ "id": "session-1", "group_id": "group-2" }`。
- `GET /api/sessions/search?query=关键词`：搜索未归档分组下的会话标题和消息，返回 ID、分组 ID、标题、模型和更新时间。
- `GET /api/sessions/history?session_id=session-1`：返回含 `sequence`、`role`、`content`、`status`、`error` 的完整历史，包括空的失败占位消息。
- `GET /api/sessions/export?session_id=session-1`：返回 `{ "content": "Markdown 文本" }`，包含消息状态和错误；文件保存由客户端处理。

启动时不自动创建空会话。React 点击“新会话”只打开本地草稿，发送第一条消息时才调用创建会话接口。直接调用 `POST /api/sessions` 仍会立即创建会话并更新服务端默认项。

创建会话的响应保留 `created` 字段，并新增返回 ID，客户端无需再依赖全局默认会话来识别创建结果：

```json
{ "created": true, "session_id": "session-1" }
```

## 聊天

`POST /api/chat`：

```json
{
  "content": "你好",
  "session_id": "session-1",
  "provider_id": "provider-1",
  "model": "deepseek-v4-flash",
  "reasoning_effort": "medium"
}
```

为兼容旧请求，四个选择字段均可省略；新客户端明确传入 `session_id`、`provider_id`、`model`。服务端在执行前固定 Provider 凭据和模型，先保存用户消息，再调用 Chat Completions；即使模型请求失败，用户消息也会保留。请求校验失败不会创建新消息。

成功响应保留原有 `message` 字段，并附带本次实际使用的 `session_id`。

`POST /api/reset` 传入 `{ "session_id": "session-1" }` 清空指定会话但保留会话本身；空请求体或 `{}` 保留旧的默认会话行为。聊天、重置、删除会话及删除分组当前串行协调，避免模型尚未结束时历史被清空后又写入迟到回复。

## 持久化

SQLite 默认使用平台用户数据目录下的 `xi/xi.db`；设置 `XI_DATA_DIR` 后改为该目录下的 `xi.db`。主要表包括：

- `providers`：Provider 配置和旧的默认模型字段；本次调用模型以聊天请求为准。
- `provider_models`：Provider 可用模型缓存。
- `conversation_groups`、`conversations`：分组和会话，带归档状态。
- `messages`、`runs`：有序消息及每次模型请求的成功、失败或中断状态。
- `app_state`：默认选择和一次性迁移标记；新消息不再写入工作区 JSON 快照。

API Key 明文保存在本地 SQLite，不会通过查询 API 返回。模型和推理强度在聊天请求中指定；不同供应商对推理强度的支持可能不同。

旧客户端数据库使用只读连接和一致性快照导入，目标写入处于同一事务。导入失败不留下半份会话，成功后通过来源路径标记避免重复导入。CLI 与桌面端使用相同表和 AppServer；CLI 流式模型输出不等于 HTTP 已实现流式事件订阅。
