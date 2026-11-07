# PROJECT STATUS

## Current Milestone

Phase 5 — End-to-End Coding Workflow（P0）。确定性闭环（探索→修改→执行→复核）已用脚本化模型与真实工具验证，单文件与双文件修复均有覆盖；模型请求已具备超时与有界重试。按开发者要求增量推进 Phase 7 的会话持久化、上下文预算与会话级失败恢复：失败或取消只回滚对话，交互会话可继续。

Phase 5.4 的“变更文件 / 执行命令 / 工具统计”报告已具备；真实模型任务集与更复杂的多文件任务尚未评估。Phase 3–7 均不标记为全部完成，路线图未改写。TUI 尚未开始，作为路线图之外的 Phase 7.5，在下列前置条件满足后启动。

## Completed

### 工程基线与配置

- 基于 `clap` 的异步 CLI 入口，统一错误输出、stderr 与失败退出码；支持 `--version`。
- 早期 Rust 练习命令及相关测试已删除；遗留的 `add` 工具及其测试已删除。
- TOML LLM 配置加载、默认值、业务校验与 `AGENT_CLI_LLM_API_KEY` fallback。

### Agent Runtime

- 消息模型：用户、assistant 最终回答、assistant 工具调用批次、工具结果。
- `AgentModel` / `ToolExecutor` 异步边界；单轮多工具调用；工具调用 ID 与结果配对。
- 多轮模型—工具循环、迭代上限、工具总次数预算；整批超限不执行，恰好达到预算仍允许最终回答。
- `ToolOutput` 区分成功、可恢复工具错误与致命执行器错误；可恢复错误以 JSON tool message 返回模型。
- `AgentEvent` 发出模型请求与工具执行的开始、结束、失败及耗时；CLI 输出到 stderr。
- 最终回答后输出工具名称、次数、总次数、Loop 次数。

### 仓库工具与工作区

- `Workspace`：真实根路径、现有路径解析、父目录越界与外部符号链接拒绝。
- `list_files`：直接子项、相对路径、类型、排序、200 项截断。
- `read_file`：UTF-8 普通文件、行范围、1 MiB 文件上限、500 行 / 32 KiB 正文上限与截断反馈。
- `search_code`：递归字面搜索、相对路径与行号、跳过生成目录与符号链接、扫描与输出限额、不完整结果反馈。
- 三个只读工具默认可用；写工具与命令工具按启动参数暴露，错误返回可恢复 `ToolOutput`。

### 写入与命令

- `replace_text`：已有 UTF-8 文件精确替换；支持 1–100 项 edits 按顺序执行，全部成功才保存一次；拒绝重复、重叠与越界；原文件与结果最多 1 MiB。
- `create_file`：父目录校验、1 MiB、不覆盖已有目标、不自动建目录、临时文件 + `persist_noclobber`。
- 保存前校验路径、内容、权限与冲突；同目录临时文件替换并保留权限；结果含状态与有界 diff。
- `--allow-write` 默认关闭；模型工具列表与执行器同时受授权约束。
- `run_command`：程序名 + 参数数组、workspace 内 cwd、默认 60 秒（1–300 秒）、并发排空输出、每流 32 KiB 保留、退出码与耗时。
- `--allow-exec` 独立授权；非零退出与超时返回可恢复失败；Unix 进程组在超时/取消时清理，真实 SIGINT 与后代清理测试通过。
- 子进程移除 `AGENT_CLI_LLM_API_KEY` 与 `OPENAI_API_KEY`。

### 变更可见性

- `ToolEffect`：`CreatedFile`、`ModifiedFile`、`CommandFinished`。
- 文件效果只在写盘成功后产生；命令效果在进程启动后产生，非零退出与超时同样报告。
- `AgentRunResult.effects` 保留有序、可重复的原始事实；CLI 按路径聚合并展示新建/修改处数，命令显示退出码或超时标记。
- 效果不发送给模型、不进入上下文预算、不写入存档；v1–v3 存档格式未变。

### 模型请求可靠性

- `ChatEndpoint` 统一 agent 与 chat 的请求路径，共享 60 秒 HTTP 超时与重试逻辑。
- 可重试：HTTP 408、429、500、502、503、504，连接失败、超时、请求中断、响应体读取失败。
- 不重试：400/401/403/404 等客户端错误、请求构建失败、2xx 但 JSON 非法。
- 默认总计 3 次尝试，确定性退避 1 s、2 s，单次退避上限 30 s；`Retry-After` 只解析整数秒并截断到 30 s。
- 重试耗尽后的错误包含尝试次数；非 2xx 错误信息为 `agent`/`chat request failed with status ...`。
- `RetryNotice` 通过 observer 交回命令层，`claw` 与 `chat` 在 stderr 提示重试次数与等待时间。
- 测试可用零延迟策略与短 HTTP 超时，不依赖真实等待或外部网络。

### 会话、上下文与交互

- `claw` 单次与 `--interactive` 连续会话；支持首条命令行 prompt、空行跳过、`/exit`、EOF、Ctrl+C（130）。
- 交互命令：`/sessions`（当前 workspace 自动存档列表）、`/resume <编号>`、`/new`；参数错误只提示、不调用模型。
- 自动存档到 `<workspace>/.agent-cli/sessions/`；空对话不写盘；正常启动不自动恢复。
- 显式 `--session-file`（新建、不覆盖）与 `--resume`（恢复并更新）互斥且要求交互模式。
- 存档 v1–v3，16 MiB 上限；加载校验版本、workspace、完整消息顺序与工具 ID/名称配对；压缩格式复用上下文引用。
- 恢复不重放旧工具，权限与预算由本次启动参数决定；未输入新请求不重写存档；失败不覆盖。
- `--max-context-bytes`（默认 256 KiB，内部消息 JSON 字节数，不是 token）：请求前按完整轮次裁剪最旧上下文并提示；输入自身超限则不发送；工具执行后超限回滚本轮并保持会话可用。
- 存档使用同目录临时文件、`sync_all` 后发布；Unix 新文件 0600；`.agent-cli/sessions/` 已忽略。

### 会话失败恢复（本次完成）

- 回合开始时在 `AgentSession` 内记录 `TurnCheckpoint`（完整历史长度 + 工作上下文快照）；成功回合提交，失败回合截断历史并恢复上下文。
- 模型失败、工具执行器致命错误、回合中上下文超限、空工具调用批次均回滚本轮并保持会话可用；失败轮不写存档。
- 只有内部不变量错误（迭代/工具计数溢出、上下文序列化失败）关闭会话并拒绝保存。
- `abort_turn()` 幂等回滚，供被 `tokio::select!` drop 的取消路径恢复内存状态。
- `ContextLimitExceeded` 携带 `Input` / `TurnInProgress` 阶段：输入前超限仍不发送，回合中超限回滚后继续。
- 对话回滚但磁盘不回滚：已写入文件与已执行命令保留，CLI 提示中明确说明。

### 文档

- `README` 重写为纯文本（Linux 项目风格、无 Markdown 标记），覆盖构建、配置、命令与参数、工具限额、权限与安全边界、会话、上下文预算、执行统计、退出码、文件布局、测试方式与已知限制。
- `SESSIONS.md` 删除已删除的 `add` 工具行；文档中的工具清单与默认工具集一致。
- 已经自动化测试覆盖的“跨进程 `--resume` 验收”不再列为手工待办。

### 测试与验证

- 最近完整测试：**221 个全部通过**（165 单元、9 chat CLI、19 Claw CLI、2 coding workflow、3 context budget、4 fetch CLI、8 session resume、11 session storage）。
- 严格 Clippy（`--all-targets -- -D warnings`）、`cargo fmt --check`、`git diff --check` 通过。
- 重试相关覆盖：503→200 重试与通知、连续 503 耗尽、400 不重试、超时后重试成功、退避与 `Retry-After` 解析、状态码分类；CLI 层 503/429 恢复。
- `tests/coding_workflow.rs` 用脚本化模型与真实工具验证单文件失败测试修复、双文件有序批量编辑；`claw_cli.rs` 新增默认工具集完整 schema 断言。
- 新增覆盖：工具结果后模型失败回滚、致命工具错误后重试、回合中超限回滚、不变量错误关闭、取消回滚幂等；CLI 层模型失败后继续输入、恢复会话失败不覆盖存档、失败后下一轮仍保存且失败轮不落盘。

## In Progress

- Phase 5：确定性场景已覆盖；真实模型、更复杂多文件任务与独立评估任务集仍未验证。
- Phase 3：无完整 patch / 多文件事务；当前是单文件精确替换与不覆盖创建。
- Phase 4：无逐命令审批、OS 沙箱与跨平台进程树清理。
- Phase 7：无流式输出、无 token 预算（仅字节预算）、无自动摘要/长期记忆；终端展示直接写在命令层。
- 执行效果尚未持久化，不能作为可回放的 trace；`run_command` 内部写入不可见。
- TUI（Phase 7.5）未开始，前置条件见 Next Step 第 4 项。

## Next Step

1. 实现逐命令审批（Phase 4.3）：交互模式提供允许/拒绝（及可选“总是允许”），非交互模式显式拒绝并返回可恢复工具错误；决策接口需能被后续 TUI 复用。
2. 抽离前端无关的事件与结果接口：补足工具参数摘要、重试、耗时、上下文裁剪与 effects 等渲染所需信息，移除 `commands/claw.rs` 中的直接终端打印。
3. 设计真实模型小型评估任务集（3–5 个固定任务，独立入口，默认不依赖外部 API），记录成功率与工具调用数。
4. TUI 设计前置条件：失败可恢复（会话级回滚已具备）且可取消当前轮（交互入口待接）、权限确认接口可用、事件流足以渲染、至少一个真实模型端到端任务通过、CLI 与 TUI 共享会话与执行器。满足后再选型（建议 `ratatui` + `crossterm`），并先定接口再写渲染。

## Architecture Notes

- `src/main.rs` 负责 Tokio 入口、命令分发与统一错误输出；`src/cli.rs` 只定义命令与参数校验。
- `src/commands/claw.rs` 组合配置、模型适配器、工具执行器、会话与 Runtime，并负责终端展示。
- `AgentSession` 跨用户请求保存完整历史与工作上下文；`AgentState` 每次请求新建，预算与统计重置。
- `run_agent` 复用会话执行逻辑，保持单次入口；交互 CLI 复用一个会话、模型和执行器，workspace 与授权固定。
- 会话执行前进入关闭态并记录检查点；成功后提交，可恢复失败或取消回滚到上一轮；只有内部不变量错误保持关闭；已落盘文件与已执行命令不回滚。
- `ToolExecutor` 返回 `Result<ToolOutput>`；`ToolOutput::error` 是可恢复失败，外层 `Err` 是致命失败。
- `ToolEffect` 由工具在成功落盘点/进程启动点声明，Runtime 只收集不解释；CLI 负责聚合与转义展示。
- 效果不进入 `AgentMessage`，因此不影响模型上下文、存档格式、恢复校验或上下文预算。
- `src/tools.rs` 负责参数解析、执行与分发；写入/搜索工具通过 `spawn_blocking` 执行。
- `src/tools/edit.rs`：`prepare_edit()` 生成方案，`apply_edit()` 校验并保存；`src/tools/create.rs` 使用 `persist_noclobber`。
- `src/tools/command.rs`：异步启动、并发收集、超时与 Unix 进程组清理。
- 权限边界在执行器：`--allow-write` 与 `--allow-exec` 是本次启动授权，不继承自存档；当前没有 OS 沙箱。
- `src/session.rs` 隔离磁盘格式与写入；`AgentSession::completed_messages()` 只提供已完成历史。
- 恢复校验遵循 Runtime 产生的消息顺序，拒绝不完整轮次与不匹配的工具结果。
- `SessionInvariant` 是私有错误标记，经 anyhow context 链判定；回滚点由 `TurnCheckpoint` 决定，回滚后工作上下文仍是完整轮次。
- `ContextLimitExceeded` 的阶段只影响 CLI 提示，不改变 Runtime 的可恢复性判断。
- `src/openai_compatible.rs`：协议 DTO、工具 schema、`ChatEndpoint` 的 HTTP 通信、超时、重试与响应解析；模型响应非流式。
- 重试可重试性分类只存在于适配器；Runtime 不感知 HTTP 状态码。重试提示通过 `RetryNotice` observer 回到命令层。
- 当前事件与统计仍由 `commands/claw.rs` 渲染；抽离后才能供 TUI 复用。
- 自动化测试不得依赖真实 LLM API、真实密钥、余额或外部 Provider 可用性。

## Open Questions

- TUI 之前是否需要流式输出，还是先接受“阶段事件 + 活动指示”的体验？
- 重试次数与模型超时是否需要暴露为配置项或 CLI 参数？是否需要随机抖动？
- 逐命令审批的最小交互集是什么（每次确认、本次总是允许、永久白名单）？
- Ctrl+C 取消当前轮后是否应留在交互会话，而不是直接退出 130？
- 执行效果是否需要持久化为结构化 trace？是否与评估统计共用格式？
- 是否需要完整 patch 与多文件事务，如何让用户方便检查实际 diff？
- 上下文控制是继续按字节预算，还是引入 token 估算与摘要压缩？
- `chat` 是否长期保留为无工具单轮入口，还是由 Claw 的受限模式替代？
- 工具数量增加后，工具定义与执行分发是否需要统一注册机制？

## Technical Debt

- 失败或取消的请求不输出统计与摘要；失败轮的 effects 只存在于内存且不会报告；效果不持久化，已完成步骤没有结构化审计。
- 失败轮若已触发上下文裁剪，回滚到裁剪前快照，下一轮会重复打印裁剪提示。
- 重试次数、超时与退避为常量，不可配置；无随机抖动。
- `Retry-After` 仅支持整数秒，不支持 HTTP-date；超时重试可能造成重复计费。
- `run_command` 内部的文件写入无法进入变更摘要；摘要不能替代完整审计。
- 尚无 Agent 总时长限制与阻塞工具取消；单次模式同步 stdin 读取不在取消范围内。
- 没有 token 预算或自动摘要；长对话仍可能超出模型窗口，只有内部 JSON 字节上限。
- 存档内容检查与替换之间仍有竞态；父目录无 fsync；阻塞保存启动后不能简单取消；存档可能含敏感历史。
- 模型响应非流式；空字符串最终回答尚未单独拒绝。
- 命令执行没有 OS 沙箱；Unix 脱离进程组的后代可能逃逸，非 Unix 仅清理直接子进程。
- 默认无次数预算；无人值守应显式配置；尚无无进展/重复失败检测。
- Workspace 存在检查与使用之间的路径替换竞态；文件工具无敏感路径排除策略。
- 编辑的最终校验与替换之间仍有并发窗口；临时文件替换仅保留当前权限。
- `list_files` 无总输出字节上限；`read_file` 正文字节上限不等于序列化后 JSON 上限。
- `search_code` 不支持 `.gitignore` 规则；条目预算耗尽时结果子集不保证跨运行一致。
- 工具定义与执行分发分别维护，集成测试仍重复部分 schema；无结构化执行追踪。
- 终端展示与 Runtime 事件耦合在 `commands/claw.rs`，阻塞 TUI 复用。
- `chat_rejects_missing_api_key` 未显式移除子进程继承的 `AGENT_CLI_LLM_API_KEY`。
- `fetch` 只限制输出字符数，没有限制响应体最大字节数。
- `Config.toml` 是本地运行配置，必须避免提交真实凭据。
- 仓库仍无 CI、LICENSE 与真实模型评估集。
- 早期 journal 存在非标准日期文件名；历史内容保留，迁移优先级低。
