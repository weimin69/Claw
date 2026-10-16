# PROJECT STATUS

## Current Milestone

Phase 1 — Agent Runtime Stabilization（收尾阶段）。

当前已经建立可通过真实 OpenAI-compatible HTTP 适配器运行的模型—工具循环，并通过 `claw` CLI 暴露给用户。进入 Phase 2 前，需要补充任务级工具调用总量限制，避免模型在单轮返回大量工具调用或跨轮持续调用工具。

## Completed

- 建立基于 `clap` 的异步 CLI 入口，并统一处理错误链、stderr 和失败退出码。
- CLI 当前提供 `claw`、`chat`、`fetch`、`read-config`，并通过 Clap 原生支持 `--version`。
- 删除早期 Rust 练习命令及相关测试，完成面向 SWE Agent 的工程基线清理。
- 实现 TOML LLM 配置加载、默认值、业务校验和 `AGENT_CLI_LLM_API_KEY` fallback。
- 实现 OpenAI-compatible 单轮 Chat Completions 请求与响应解析。
- 实现 Agent Runtime：
  - 用户消息、assistant 工具调用和工具结果历史。
  - 最终回答和单轮多个工具调用决策。
  - 异步 `AgentModel` 与 `ToolExecutor` 边界。
  - 多轮模型—工具循环和最大迭代次数限制。
  - 工具调用 ID 与结果关联。
- 实现 `ToolOutput`，区分成功、可恢复工具错误和致命执行器错误。
- 实现结构化 JSON `add` 工具，包括参数错误、未知工具和整数溢出处理。
- 实现 `OpenAiCompatibleAgentModel::next_decision()`：
  - 复用带 60 秒 timeout 的 HTTP client。
  - 构造带工具定义和消息历史的请求。
  - 检查 HTTP status、读取响应体并解析模型决策。
  - 支持单轮一个或多个 tool calls。
- 将可恢复工具错误编码为 JSON tool message 返回模型，允许模型修正调用或解释失败。
- 新增用户可见的 `claw` 命令：
  - 支持命令行 prompt 和 stdin fallback。
  - 支持 `--max-iterations`，默认值为 8。
  - 加载 LLM 配置并运行真实 Agent Loop。
- 使用确定性单元测试和 Wiremock 测试覆盖 HTTP、协议转换、单/多工具执行、错误处理、运行限制和 Claw CLI 完整闭环。
- 当前验证结果：`cargo fmt --check`、`cargo check` 和完整 `cargo test` 通过，共 69 个测试。

## In Progress

- Phase 1 运行限制收尾：当前限制模型迭代次数，但尚未独立限制单轮及任务级工具调用总数。
- Clippy 严格检查仍被 `config.rs` 中一个既有的 `collapsible_if` 阻塞。

## Next Step

为 Claw 增加任务级工具调用数量限制：

1. 为 `run_agent()` 增加 `max_tool_calls` 配置或执行限制结构。
2. 在执行每批 tool calls 前检查累计数量，超限时安全终止且不执行部分批次。
3. 添加零值、边界值、单轮批量超限和跨轮累计超限测试。
4. 将限制接入 `claw` CLI，并提供合理默认值。
5. 修复既有 Clippy `collapsible_if`，运行完整格式、编译、测试和严格 Clippy 检查。

完成这些运行安全项后，进入 Phase 2，设计 active workspace 并实现 `list_files`。

## Architecture Notes

- `src/main.rs` 负责 Tokio 入口、CLI 命令分发和统一错误输出。
- `src/cli.rs` 只定义用户可见命令与参数校验。
- `src/commands/claw.rs` 负责组合配置、模型适配器、工具执行器和 Agent Runtime。
- `src/agent.rs` 负责消息模型、Agent 状态、执行循环、迭代限制及模型和工具边界。
- `ModelDecision::ToolCalls(Vec<ToolCall>)` 表示一次 assistant 响应中的整批工具调用。
- `AgentMessage::AssistantToolCalls` 保留单次模型响应的协议语义；同一批调用不会拆成多条 assistant 消息。
- `ToolExecutor` 返回 `Result<ToolOutput>`：`ToolOutput::error` 是可恢复失败，外层 `Err` 是致命执行器失败。
- `src/tools.rs` 负责内置工具参数解析、执行和名称分发。
- `src/openai_compatible.rs` 负责协议 DTO、请求构造、HTTP 通信和响应解析。
- 自动化测试不得依赖真实 LLM API、真实 API key、余额或外部 Provider 可用性。

## Open Questions

- 工具调用限制应只有任务级总量，还是同时限制单轮批量大小？
- `chat` 是否长期保留为无工具单轮入口，还是最终由 Claw 的受限聊天模式替代？
- `fetch` 和 `read-config` 在 Coding Agent 工具成熟后是否继续作为用户命令保留？
- 工具数量增加后，工具定义与执行分发是否需要统一注册机制？

## Technical Debt

- 尚未实现 Agent 级 timeout、取消和独立的最大工具调用次数。
- 尚未实现 `list_files`、`read_file`、`search_code` 等仓库探索工具。
- `config.rs` 存在一个导致 `cargo clippy --all-targets -- -D warnings` 失败的 `collapsible_if`。
- `chat_rejects_missing_api_key` 未显式移除子进程继承的 `AGENT_CLI_LLM_API_KEY`，可能受宿主环境影响。
- `fetch` 只限制输出字符数，没有限制下载响应体的最大字节数。
- `Config.toml` 是本地运行配置，必须避免提交真实凭据；后续应提供安全的示例配置策略。
- README 尚未记录 `claw` 命令、Agent Loop、运行限制和当前工具能力。
- 早期 journal 文件存在非标准日期文件名；历史内容应保留，文件名迁移优先级较低。
