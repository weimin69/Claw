# PROJECT STATUS

## Current Milestone

Phase 1 — Agent Runtime Stabilization.

当前目标是在已有最小 Agent Loop 基础上，完成真实 OpenAI-compatible 模型适配器，建立稳定、可测试的模型—工具执行链路，并为后续 CLI 接入做好准备。

Phase 0 的工程基线清理已经完成：早期 Rust 练习命令及其测试已从产品代码中移除，CLI 目前只保留与项目方向相关的命令。

## Completed

- 建立基于 `clap` 的异步 CLI 入口，并统一处理错误链、stderr 和失败退出码。
- CLI 当前提供 `chat`、`fetch`、`read-config`，并通过 Clap 原生支持 `--version`。
- 删除 `hello`、`echo`、`divide`、`sum`、`repeat`、`wait`、`parallel-wait` 和旧 `version` 子命令及相关测试。
- 实现 TOML LLM 配置加载、默认值、业务校验和错误上下文。
- 支持通过 `AGENT_CLI_LLM_API_KEY` 提供 API key fallback，且显式 TOML 配置优先。
- `read-config` 只报告 API key 是否设置，不输出真实密钥。
- 实现 OpenAI-compatible 单轮 Chat Completions 请求与响应解析。
- `chat` 支持命令行 prompt 和 stdin fallback，并验证空白输入、API key 和 Provider 错误。
- 实现带 timeout、HTTP status 检查和 Unicode 字符截断的 `fetch` 命令。
- 实现最小 Agent Runtime：
  - `AgentMessage` 消息历史。
  - `ModelDecision::FinalAnswer` 和单个 `ToolCall`。
  - 异步 `AgentModel` 与 `ToolExecutor` 边界。
  - 模型—工具—模型循环。
  - 最大迭代次数限制。
  - 工具调用 ID 与工具结果关联。
- 实现最小 `BuiltinToolExecutor` 和结构化 JSON `add` 工具，包括参数错误、未知工具和整数溢出处理。
- 实现 Agent 消息到 OpenAI-compatible 消息的映射。
- 实现 `add` function tool 定义及 JSON Schema。
- 实现 Agent 响应到 `FinalAnswer` 或 `ToolCall` 的解析；当前明确拒绝单轮多个工具调用。
- 建立 `OpenAiCompatibleAgentModel` 结构体，并在构造时创建可复用、带 60 秒 timeout 的 `reqwest::Client`。
- 使用确定性单元测试和 Wiremock CLI 集成测试覆盖核心成功与错误路径。
- 当前验证结果：`cargo fmt --check`、`cargo check` 和完整 `cargo test` 通过，共 56 个测试。

## In Progress

- `OpenAiCompatibleAgentModel::next_decision()` 已完成第一版实现，能够复用现有 HTTP client、Agent 请求构造器、HTTP status 检查和响应决策解析器。
- 第一版实现已经通过编译和现有协议单元测试，但尚未完成格式整理、警告清理和适配器 HTTP 行为测试，因此仍属于未完成状态。
- 正在为真实模型适配器补充不依赖外部 Provider 的 Wiremock 测试。

## Next Step

整理并验证 `OpenAiCompatibleAgentModel` 的第一版 `AgentModel` 实现：

1. 移除未使用的 `reqwest::Client` import，并运行 `cargo fmt`。
2. 复查请求发送、HTTP status、响应体读取和响应解析错误边界。
3. 使用 Wiremock 验证最终回答、工具调用、请求路径、认证 header 和请求体。
4. 增加非 2xx 和非法响应测试。
5. 运行完整格式、编译和测试验证。

适配器通过测试后，再评估将 Agent Runtime 接入新的 CLI 命令。

## Architecture Notes

- `src/main.rs` 负责 Tokio 入口、CLI 命令分发和统一错误输出。
- `src/cli.rs` 只定义用户可见的命令与参数校验。
- `src/commands/` 负责当前用户可见命令的输入选择和输出行为。
- `src/config.rs` 负责配置解析、默认值、环境变量合并和通用校验。
- `src/agent.rs` 负责 Agent 状态、执行循环、迭代限制以及模型和工具行为边界。
- `src/tools.rs` 负责内置工具参数解析、执行和名称分发。
- `src/openai_compatible.rs` 负责 OpenAI-compatible 协议 DTO、请求构造、HTTP 通信和响应解析。
- 当前继续使用泛型 `AgentModel` / `ToolExecutor`，在没有 trait object 需求前不引入额外抽象。
- Provider 数量增加不等于协议实现数量增加；目前只维护一个 OpenAI-compatible 协议实现。
- 自动化测试不得依赖真实 LLM API、真实 API key、余额或外部 Provider 可用性。

## Open Questions

- Agent CLI 应新增独立的 `agent` 命令，还是逐步替代当前单轮 `chat` 命令？
- 可恢复工具失败应如何编码为 `ToolResult` 并返回模型，而不是直接终止 Agent Loop？
- 何时将 `ModelDecision` 扩展为支持单轮多个工具调用？
- `fetch` 和 `read-config` 在 Agent CLI 成熟后是否继续作为用户命令保留？
- 工具数量增加后，工具定义与执行分发是否需要统一注册机制？

## Technical Debt

- Agent Runtime 尚未接入 CLI，普通构建存在相关 dead-code 警告。
- `OpenAiCompatibleAgentModel` 的第一版实现尚未通过格式检查，并存在一个未使用 import。
- 真实模型适配器尚无 Wiremock HTTP 行为测试，现有测试只覆盖请求构造和响应解析等纯逻辑。
- 当前工具失败会终止 Agent Loop，尚未区分可恢复工具失败与致命运行时错误。
- 当前一次模型决策只能表达一个工具调用。
- 尚未实现 Agent 级 timeout、取消和最大工具调用次数。
- 尚未实现 `list_files`、`read_file`、`search_code` 等仓库探索工具。
- `chat_rejects_missing_api_key` 未显式移除子进程继承的 `AGENT_CLI_LLM_API_KEY`，可能受宿主环境影响。
- `fetch` 只限制输出字符数，没有限制下载响应体的最大字节数。
- `Config.toml` 是本地运行配置，需要确认是否包含真实凭据以及是否应改为示例配置。
- `README` 的 Markdown 代码块和环境变量说明仍需整理。
- 早期 journal 文件存在非标准日期文件名；历史内容应保留，文件名迁移优先级较低。
