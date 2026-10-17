# PROJECT STATUS

## Current Milestone

Phase 2 — Repository Awareness（核心实现已完成，待整体验收）。

Agent 已具备 workspace 内列目录、读取文件和搜索代码能力，并通过确定性工具测试和 CLI 闭环测试。Phase 1 的可选运行预算、Ctrl+C 取消与严格 Clippy 收尾已实施；未设置预算时不会因次数终止，这是当前明确的交互式运行策略。下一步先审查并综合验收仓库探索能力，再确定 Phase 3 的最小安全编辑接口。

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
  - 支持可选 `--max-iterations` 和 `--max-tool-calls`，默认无次数预算，显式零值被拒绝。
  - 支持 Ctrl+C 取消 Agent 异步等待，stderr 报告取消并以 130 退出。
  - 支持 `--workspace`，默认当前目录，启动时校验并规范化真实路径。
  - 加载 LLM 配置并运行真实 Agent Loop。
- 使用确定性单元测试和 Wiremock 测试覆盖 HTTP、协议转换、单/多工具执行、错误处理、运行限制和 Claw CLI 完整闭环。
- 工具预算执行前检查累计数量，整批超限不执行，恰好达到预算后仍允许最终回答。
- 实现 Workspace：根目录校验、现有路径解析、父目录越界和外部符号链接拒绝。
- 实现 `list_files`：直接子项、相对路径、文件类型、排序及 200 项截断。
- 实现 `read_file`：UTF-8 普通文件、指定行范围、1 MiB 文件上限、500 行 / 32 KiB 正文上限及截断反馈。
- 实现 `search_code`：递归字面搜索、相对路径与行号、跳过生成目录和遍历符号链接、扫描和输出限额、不完整结果反馈。
- 三个仓库工具的输入和访问错误返回可恢复 ToolOutput；工具定义已加入模型请求。
- 修复 `config.rs` 既有 `collapsible_if`。
- 最近完整验证：104 个测试全部通过；`cargo fmt --check`、`cargo check`、严格 Clippy 和 `git diff --check` 通过。

## In Progress

- Phase 2 整体验收与开发者代码审查尚未完成；手动列目录已验证，完整搜索—阅读综合任务未记录。
- Phase 3 编辑工具仅讨论方向，尚无实现；精确替换与 patch 协议、授权和冲突处理仍待确定。

## Next Step

1. 审查 `read_file` 等工具从输入校验、workspace 检查到结果回写的完整路径。
2. 对真实 Rust 仓库执行探索—搜索—阅读综合验收，记录当前限制与验收结果。
3. 为 Phase 3 确定最小编辑接口：已有文件精确替换是候选方案；明确授权、匹配冲突、用户改动保护和 diff 展示后再实施。

## Architecture Notes

- `src/main.rs` 负责 Tokio 入口、CLI 命令分发和统一错误输出。
- `src/cli.rs` 只定义用户可见命令与参数校验。
- `src/commands/claw.rs` 负责组合配置、模型适配器、工具执行器和 Agent Runtime。
- `src/agent.rs` 负责消息模型、Agent 状态、执行循环、迭代限制及模型和工具边界。
- `ModelDecision::ToolCalls(Vec<ToolCall>)` 表示一次 assistant 响应中的整批工具调用。
- `AgentMessage::AssistantToolCalls` 保留单次模型响应的协议语义；同一批调用不会拆成多条 assistant 消息。
- `ToolExecutor` 返回 `Result<ToolOutput>`：`ToolOutput::error` 是可恢复失败，外层 `Err` 是致命执行器失败。
- `src/tools.rs` 负责内置工具参数解析、执行和名称分发。
- `BuiltinToolExecutor` 持有 Workspace；`src/workspace.rs` 集中处理真实根路径和访问边界。
- `src/tools/search.rs` 实现有界字面搜索。文件系统工具通过 `spawn_blocking` 执行。
- Workspace 默认当前目录，不修改进程工作目录；路径检查不等同于操作系统沙箱。
- 执行轮数和工具总次数为独立可选预算；Ctrl+C 在命令层控制当前 Agent Future。
- `src/openai_compatible.rs` 负责协议 DTO、请求构造、HTTP 通信和响应解析。
- 自动化测试不得依赖真实 LLM API、真实 API key、余额或外部 Provider 可用性。

## Open Questions

- 编辑工具采用何种最小接口与补丁格式，如何实现授权、冲突检测和可检查 diff？
- 工具调用及结果摘要应如何展示，便于用户核对 Agent 实际执行的操作？
- `chat` 是否长期保留为无工具单轮入口，还是最终由 Claw 的受限聊天模式替代？
- `fetch` 和 `read-config` 在 Coding Agent 工具成熟后是否继续作为用户命令保留？
- 工具数量增加后，工具定义与执行分发是否需要统一注册机制？

## Technical Debt

- 尚无 Agent 总时长限制、阻塞工具取消和外部子进程终止机制；同步 stdin 读取不在当前取消范围内。
- 默认无次数预算；无人值守使用应显式配置，自动无进展检测尚未实现。
- Workspace 存在检查与使用之间的路径替换竞态，文件工具尚无敏感路径排除策略。
- `list_files` 尚无总输出字节上限；`read_file` 正文字节上限不等于序列化后 JSON 上限。
- `search_code` 暂无 .gitignore 规则支持；条目预算耗尽时为部分遍历，结果子集不保证跨运行一致。
- 工具定义与执行分发分别维护，集成测试仍重复部分 schema；工具追踪输出尚未实现。
- `chat_rejects_missing_api_key` 未显式移除子进程继承的 `AGENT_CLI_LLM_API_KEY`，可能受宿主环境影响。
- `fetch` 只限制输出字符数，没有限制下载响应体的最大字节数。
- `Config.toml` 是本地运行配置，必须避免提交真实凭据；后续应提供安全的示例配置策略。
- README 尚未记录 `claw` 命令、Agent Loop、运行限制和当前工具能力。
- 早期 journal 文件存在非标准日期文件名；历史内容应保留，文件名迁移优先级较低。
