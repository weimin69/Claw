# PROJECT STATUS

## Current Milestone

Phase 5 — End-to-End Coding Workflow（基础修复闭环与确定性综合回归已验证）；按开发者要求增量推进 Phase 7 会话能力。

Agent 已支持仓库探索、文件编辑、创建、显式授权的命令执行、连续会话、进度耗时和显式磁盘存档。基础修复综合回归使用模拟模型与真实工具，不代表复杂任务的模型能力评估。会话恢复、上下文压缩、TUI 和逐次审批尚未实现；不标记 Phase 3–7 全部完成，路线图未改写。

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
- 实现 `replace_text`：已有 UTF-8 普通文件唯一精确替换，拒绝重复和重叠匹配；仅空文件允许空原文初始化，原文件与结果最多 1 MiB。
- 实现 EditPlan 和 diff；保存前校验路径、内容及权限，通过同目录临时文件替换并保留文件权限。
- `--allow-write` 默认关闭；模型工具列表和执行器同时根据授权控制写入，结果含状态及有界 diff。
- 实现 `create_file`：父目录验证、UTF-8 初始内容、1 MiB 限制、临时文件和不覆盖保存；拒绝已有文件、目录和符号链接，不自动创建目录。
- 实现 `run_command`：程序名/参数数组、workspace 内 cwd、默认 60 秒超时（可选 1–300 秒）、并发排空输出、每个流 32 KiB 保留上限、退出码和耗时反馈。
- `--allow-exec` 独立授权执行；默认不声明命令工具且执行器拒绝越权调用，非零退出和超时返回可恢复失败。
- Unix 命令进程组在超时和取消时清理，直接子进程启用 kill_on_drop；真实 SIGINT 与后代清理测试通过。
- 独立 `claw-repair-demo` 的基础修复任务：开发者提供失败测试与修复记录，当时独立核对实现为加法、断言未改，并重新运行离线测试通过；仅代表一个小型场景。
- 最终回答后打印 Runtime 实际记录的工具名称、各工具次数、总次数及 Loop 次数；包含最终回答轮和可恢复工具失败，仅正常结束时输出。
- `claw --interactive` 支持同一进程内连续输入；保存用户、最终回答、工具调用及结果，支持可选首条命令行 prompt、空行跳过、`/exit`、EOF 和 Ctrl+C。
- 开发者报告连续会话手动验收通过。
- Runtime 通过 `AgentEvent` 回调发出模型请求与工具执行的开始、结束、失败及耗时；CLI 输出到 stderr，不输出工具参数与正文，不污染模型上下文或最终 stdout。
- `tests/coding_workflow.rs` 在临时无依赖 Rust 项目中使用模拟 HTTP 模型与真实工具验证“测试失败→读取源码→修复→测试成功”；核对调用历史、磁盘实现及未修改的断言，并独立再次运行离线测试。
- `SessionFile` / 消息 serde 支持带版本、规范化 workspace 与完整历史的 JSON；`--interactive --session-file <新路径>` 每次请求成功后保存完整快照，不指定就不写盘。
- 存档首次拒绝已有文件，后续检查本次写入内容；同目录临时文件写入、sync_all 后发布，Unix 使用 0600 权限。保存失败明确报错并结束运行，请求失败/取消保留最后完成快照。
- 最近完整测试为 156 个全部通过（121 单元、8 chat CLI、16 Claw CLI、1 coding workflow、4 fetch CLI、6 session storage）；严格 Clippy、修改文件格式检查与 `git diff --check` 通过。全仓格式仍有 `search.rs` 既有问题，本次未修复或宣称全仓格式通过。

## In Progress

- 基础真实模型修复场景与确定性综合回归已核验；更复杂、多文件任务与真实模型任务集仍未评估。
- Phase 3 尚无完整 patch、多文件事务和独立终端 diff 检查；当前有单文件精确替换与不覆盖创建。
- Phase 4 尚无 OS 沙箱、逐命令审批、跨平台进程树清理；复杂真实项目修复能力仍未验收。
- 会话保存已有自动化验证，尚无开发者手动存档验收记录；加载、恢复、版本/历史校验和上下文增长控制待实现。

## Next Step

1. 手动验收 `--interactive --session-file <新路径>`：两轮对话后检查完整 JSON、退出后存档保留、已有路径不覆盖。
2. 实现有界加载和恢复校验：格式版本、workspace、消息顺序及工具调用配对；接入显式 `--resume`，默认仍新会话，权限由本次参数决定。
3. 再设计上下文长度控制；摘要、长期项目记忆及向量检索不与原始存档混为一谈。
4. 处理 `search.rs` 格式差异并完善用户文档；TUI、斜杠菜单和逐次交互审批继续后置。

## Architecture Notes

- `src/main.rs` 负责 Tokio 入口、CLI 命令分发和统一错误输出。
- `src/cli.rs` 只定义用户可见命令与参数校验。
- `src/commands/claw.rs` 负责组合配置、模型适配器、工具执行器和 Agent Runtime。
- `src/agent.rs` 负责消息模型、Agent 状态、执行循环、迭代限制及模型和工具边界。
- `AgentSession` 跨用户请求保存内存历史；`AgentState` 每次请求新建，预算和统计重置。最终回答作为 `AgentMessage::Assistant` 写回历史，显示用统计不写入模型上下文。
- `run_agent` 复用会话执行逻辑，保持原单次入口；交互 CLI 复用一个会话、模型和执行器，workspace 与授权保持固定。
- 会话执行前关闭、成功后恢复；失败或取消后不可复用，CLI 结束会话，避免发送不完整工具消息。已落盘文件不自动回滚。
- 交互 stdin 通过独立线程和有界通道逐行读取，等待输入不阻塞异步运行；提示写 stderr，回答与统计写 stdout。
- `src/session.rs` 隔离磁盘格式与写入；`AgentSession::completed_messages()` 只提供已完成历史，CLI 通过 `spawn_blocking` 保存后才进入下一次输入。
- 显式存档参数只用于交互模式；路径相对进程 cwd，父目录必须存在，不自动恢复已有文件。存档不包含配置凭据字段或执行授权，但历史仍可能含敏感信息。
- `ModelDecision::ToolCalls(Vec<ToolCall>)` 表示一次 assistant 响应中的整批工具调用。
- `AgentMessage::AssistantToolCalls` 保留单次模型响应的协议语义；同一批调用不会拆成多条 assistant 消息。
- `ToolExecutor` 返回 `Result<ToolOutput>`：`ToolOutput::error` 是可恢复失败，外层 `Err` 是致命执行器失败。
- `src/tools.rs` 负责内置工具参数解析、执行和名称分发。
- `BuiltinToolExecutor` 持有 Workspace；`src/workspace.rs` 集中处理真实根路径和访问边界。
- `src/tools/search.rs` 实现有界字面搜索。文件系统工具通过 `spawn_blocking` 执行。
- `src/tools/edit.rs`：`prepare_edit()` 生成方案，`apply_edit()` 校验并保存，`execute()` 返回有界 JSON 结果。
- `src/tools/create.rs`：新目标父目录检查、临时写入、`persist_noclobber` 不覆盖发布。
- `src/tools/command.rs`：异步启动程序、并发收集输出、超时和进程生命周期；Unix RAII guard 终止独立进程组。
- 模型与执行器默认只读，`--allow-write` 是本次任务授权；执行器检查是实际权限边界。当前 diff 返回模型，并非用户保存前批准。
- `--allow-exec` 是独立的任意执行授权，命令可写文件且不受 `--allow-write` 约束。cwd 校验不限制程序访问宿主文件和网络，当前没有 OS 沙箱。
- 命令 stdin 关闭；子进程移除 AGENT_CLI_LLM_API_KEY 和 OPENAI_API_KEY，其他环境与文件凭据并未隔离。
- Workspace 默认当前目录，不修改进程工作目录；路径检查不等同于操作系统沙箱。
- 执行轮数和工具总次数为独立可选预算；Ctrl+C 在命令层控制当前 Agent Future。
- `src/openai_compatible.rs` 负责协议 DTO、请求构造、HTTP 通信和响应解析。
- 自动化测试不得依赖真实 LLM API、真实 API key、余额或外部 Provider 可用性。

## Open Questions

- 是否需要完整 patch 和多文件操作，如何让用户方便检查实际 diff？
- 在现有粗粒度执行授权之后，何时引入命令审批或 OS 沙箱及跨平台进程树管理？
- 现有阶段耗时基础上，何时接入流式输出及更丰富的操作摘要？
- 会话持久化与恢复如何绑定 workspace/权限，如何限制和压缩长历史？
- `chat` 是否长期保留为无工具单轮入口，还是最终由 Claw 的受限聊天模式替代？
- `fetch` 和 `read-config` 在 Coding Agent 工具成熟后是否继续作为用户命令保留？
- 工具数量增加后，工具定义与执行分发是否需要统一注册机制？

## Technical Debt

- 尚无 Agent 总时长限制和阻塞工具取消；单次模式同步 stdin 读取不在当前取消范围内，交互等待输入已支持 Ctrl+C。
- 已支持显式保存，尚无恢复、存档大小限制、上下文长度限制或压缩；长期对话可能超过模型限制。第一版错误/取消结束会话，不支持原地重试。
- 存档内容检查与替换之间仍有竞态，不是恶意并发防护；父目录无 fsync，不承诺断电后的完整持久性。阻塞保存启动后不能简单取消，存档可能含敏感历史，禁止默认提交 Git。
- 模型响应仍非流式；空字符串最终回答尚未单独拒绝。工具已写入后取消不会撤销更改，也不打印最终统计。
- 命令执行没有 OS 沙箱；Unix 脱离进程组的后代可能逃逸，非 Unix 仅直接子进程清理，尚未验证跨平台进程树终止。
- 默认无次数预算；无人值守使用应显式配置，自动无进展检测尚未实现。
- Workspace 存在检查与使用之间的路径替换竞态，文件工具尚无敏感路径排除策略。
- 编辑的最终校验与替换之间仍存在并发窗口；临时文件替换仅保留当前处理的权限，不保证全部元数据保留。
- 阻塞写入启动后不能由 Ctrl+C 保证取消或回滚；尚无逐次写入审批，TUI 后置。
- `src/tools/search.rs` 的已有空白差异导致全仓 `cargo fmt --check` 失败，待格式化。
- `list_files` 尚无总输出字节上限；`read_file` 正文字节上限不等于序列化后 JSON 上限。
- `search_code` 暂无 .gitignore 规则支持；条目预算耗尽时为部分遍历，结果子集不保证跨运行一致。
- 工具定义与执行分发分别维护，集成测试仍重复部分 schema；已有开始/结束事件，但尚无持久化结构化执行追踪。
- `chat_rejects_missing_api_key` 未显式移除子进程继承的 `AGENT_CLI_LLM_API_KEY`，可能受宿主环境影响。
- `fetch` 只限制输出字符数，没有限制下载响应体的最大字节数。
- `Config.toml` 是本地运行配置，必须避免提交真实凭据；后续应提供安全的示例配置策略。
- README 尚未记录 `claw` 命令、Agent Loop、运行限制和当前工具能力。
- 早期 journal 文件存在非标准日期文件名；历史内容应保留，文件名迁移优先级较低。
