# PROJECT STATUS

## Current Milestone

Phase 3 — Code Editing（已有文件精确替换已接通，阶段仍在进行）。

Agent 的目录探索、读取和搜索已经通过自动化闭环测试，开发者提供的真实仓库分析结果经源码核对基本准确，但尚无工具日志证明具体调用序列。当前已实现 `replace_text` 和显式写授权；下一步在临时项目手动验收编辑行为，并确定 Phase 3 的新建文件与 diff 检查等剩余范围。Phase 3 尚未全部完成，TUI 和逐次交互确认按用户决定后置。

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
- 实现 `replace_text`：已有 UTF-8 普通文件唯一精确替换，拒绝空原文、重复和重叠匹配，原文件与结果最多 1 MiB。
- 实现 EditPlan 和 diff；保存前校验路径、内容及权限，通过同目录临时文件替换并保留文件权限。
- `--allow-write` 默认关闭；模型工具列表和执行器同时根据授权控制写入，结果含状态及有界 diff。
- 最近完整测试为 118 个全部通过；收尾编译、严格 Clippy 和 `git diff --check` 通过。全仓格式检查因 `search.rs` 已有空白差异未通过。

## In Progress

- 已有文件编辑的自动化闭环已验证，真实模型在临时项目中编辑的手动验收尚未记录。
- Phase 3 尚无新建文件、完整 patch、多文件操作和独立终端 diff 检查；当前能力只覆盖单文件精确替换。
- 尚未实现 shell 工具，无法由 Agent 自主运行构建和测试。

## Next Step

1. 在临时项目验收默认只读与 `--allow-write`，对照工具结果和实际改动，不直接用当前开发仓库作试写目标。
2. 处理 `src/tools/search.rs` 格式差异，再运行全仓格式检查。
3. 确定 Phase 3 新建文件与可审查 diff 的剩余实现范围；随后设计 shell 的权限、超时、输出和子进程管理。
4. TUI、斜杠菜单和逐次写入确认放在后续交互会话阶段。

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
- `src/tools/edit.rs`：`prepare_edit()` 生成方案，`apply_edit()` 校验并保存，`execute()` 返回有界 JSON 结果。
- 模型与执行器默认只读，`--allow-write` 是本次任务授权；执行器检查是实际权限边界。当前 diff 返回模型，并非用户保存前批准。
- Workspace 默认当前目录，不修改进程工作目录；路径检查不等同于操作系统沙箱。
- 执行轮数和工具总次数为独立可选预算；Ctrl+C 在命令层控制当前 Agent Future。
- `src/openai_compatible.rs` 负责协议 DTO、请求构造、HTTP 通信和响应解析。
- 自动化测试不得依赖真实 LLM API、真实 API key、余额或外部 Provider 可用性。

## Open Questions

- 如何增量支持新建文件及更完整 patch，并让用户方便检查实际 diff？
- shell 的授权范围、默认超时、输出限额和子进程终止策略如何确定？
- 工具调用及结果摘要应如何展示，便于用户核对 Agent 实际执行的操作？
- `chat` 是否长期保留为无工具单轮入口，还是最终由 Claw 的受限聊天模式替代？
- `fetch` 和 `read-config` 在 Coding Agent 工具成熟后是否继续作为用户命令保留？
- 工具数量增加后，工具定义与执行分发是否需要统一注册机制？

## Technical Debt

- 尚无 Agent 总时长限制、阻塞工具取消和外部子进程终止机制；同步 stdin 读取不在当前取消范围内。
- 默认无次数预算；无人值守使用应显式配置，自动无进展检测尚未实现。
- Workspace 存在检查与使用之间的路径替换竞态，文件工具尚无敏感路径排除策略。
- 编辑的最终校验与替换之间仍存在并发窗口；临时文件替换仅保留当前处理的权限，不保证全部元数据保留。
- 阻塞写入启动后不能由 Ctrl+C 保证取消或回滚；尚无逐次写入审批，TUI 后置。
- `src/tools/search.rs` 的已有空白差异导致全仓 `cargo fmt --check` 失败，待格式化。
- `list_files` 尚无总输出字节上限；`read_file` 正文字节上限不等于序列化后 JSON 上限。
- `search_code` 暂无 .gitignore 规则支持；条目预算耗尽时为部分遍历，结果子集不保证跨运行一致。
- 工具定义与执行分发分别维护，集成测试仍重复部分 schema；工具追踪输出尚未实现。
- `chat_rejects_missing_api_key` 未显式移除子进程继承的 `AGENT_CLI_LLM_API_KEY`，可能受宿主环境影响。
- `fetch` 只限制输出字符数，没有限制下载响应体的最大字节数。
- `Config.toml` 是本地运行配置，必须避免提交真实凭据；后续应提供安全的示例配置策略。
- README 尚未记录 `claw` 命令、Agent Loop、运行限制和当前工具能力。
- 早期 journal 文件存在非标准日期文件名；历史内容应保留，文件名迁移优先级较低。
