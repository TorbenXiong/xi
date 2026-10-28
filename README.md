# xi

## 当前技术方向

xi 采用 **Rust Agent Harness + Tokio** 作为核心运行时，保持 Agent Runtime、Execution Engine 和客户端解耦。

- 桌面端：Tauri 2 + React + TypeScript
- Web 端：React + TypeScript，作为远程控制入口
- 移动端：暂定 Tauri 2 或 Flutter，先验证需求
- 本地持久化：SQLite；远程多用户阶段再引入 PostgreSQL
- 会话管理：SQLite 持久化会话、消息、分组和模型运行状态
- 协议：JSON-RPC 2.0 + 事件流；外部工具接入 MCP；ACP 后续评估
- 第一阶段：先实现 Rust Runtime 和 CLI，再做桌面客户端

文档统一放在 [`docs/`](docs/)；学习内容位于 [`docs/learning/`](docs/learning/)：

- [00-技术选型](docs/learning/00-技术选型.md)
- [01-单次对话](docs/learning/01-单次对话.md)
- [02-循环对话](docs/learning/02-循环对话.md)
- [03-会话持久化与分组](docs/learning/03-会话持久化与分组.md)
- [会话持久化与分组方案](docs/方案-会话持久化与分组.md)
