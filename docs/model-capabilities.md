# 模型推理能力

核对日期：2026-10-29。

推理档位属于具体模型和服务协议，不能作为全局固定枚举。当前传输仅支持 Chat Completions 的顶层 reasoning_effort；客户端选项与后端验证均来自所选模型描述。

## 能力来源

1. /models 若在 data[] 声明 reasoning_efforts，采用声明，标记 provider。这是可选扩展，不是标准模型列表保证提供的字段。
2. 未声明时，仅对下表精确 ID 使用核实的预设，标记 builtin。
3. 其余型号标记 unknown，能力数组为空。界面显示“服务默认”，不发送推理参数。
4. 用户可在模型服务设置中逐模型配置，保存为 custom，覆盖声明和预设；刷新保留同模型自定义能力，移除下架型号。

| 精确模型 ID | 内置选项 | 官方依据 |
| --- | --- | --- |
| gpt-5.2 | none, low, medium, high, xhigh | [GPT-5.2](https://developers.openai.com/api/docs/models/gpt-5.2) |
| gpt-5.4 | none, low, medium, high, xhigh | [GPT-5.4](https://developers.openai.com/api/docs/models/gpt-5.4) |
| gpt-5.5 | none, low, medium, high, xhigh | [GPT-5.5](https://developers.openai.com/api/docs/models/gpt-5.5) |
| deepseek-flash, deepseek-v4-pro | low, high, max | [DeepSeek Thinking Mode](https://api-docs.deepseek.com/guides/thinking_mode/) |

OpenAI 型号允许 openai/ 前缀。快照、其他别名和仅含相似前缀的型号不会猜测能力，按未知型号处理。预设需随官方资料维护，代理有差异时应使用该服务自定义配置。

## 默认与显式档位

“默认”表示省略参数，不强制指定厂商默认值。none 是明确发送的值；DeepSeek 的思考开关属于另一字段，不能把 none 当作通用关闭值。切服务、模型或新会话时恢复默认，客户端过滤和后端验证阻止发送不支持的档位。

自定义项需非空、不重复、最多 32 字符，字符仅允许 ASCII 字母数字、_、-。auto 保留给本地默认行为，不能作为实际档位保存。空数组表示仅使用服务默认。

## 支持边界

[OpenAI 推理指南](https://developers.openai.com/api/docs/guides/reasoning?api-mode=chat)说明支持值依模型而异，不能把一个通用枚举套给所有型号。当前没有可靠的跨供应商标准能力发现接口，因此保留能力来源和逐模型配置。

[Anthropic Effort](https://platform.claude.com/docs/en/build-with-claude/effort)使用原生 Messages 的 output_config.effort，不能据此假设 Chat Completions 代理支持同名顶层参数。当前未实现 Anthropic 原生 Messages 或 OpenAI Responses。Claude 等代理型号默认未知，确认代理支持后才应自定义选项。

实现入口：src/model.rs、src/storage/providers.rs、app/src/client/models.ts。测试使用本机 mock 服务验证元数据、自定义覆盖、非法档位拒绝及参数省略，不代表已实测真实供应商。
