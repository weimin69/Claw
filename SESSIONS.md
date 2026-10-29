# CLI 与会话使用说明

本文对应当前实现。项目中的文件名是 `SESSIONS.md`。

## 命令总览

开发时使用 `cargo run -- <命令>`；编译后可将 `cargo run --` 替换为
`./target/debug/agent-cli-rust`。下文的 `Config.toml` 是示例配置路径。

| 命令 | 用途 |
| --- | --- |
| `claw <CONFIG_PATH> [PROMPT]` | 运行可调用工具的 Coding Agent，支持单次和交互模式 |
| `chat <CONFIG_PATH> [PROMPT]` | 无工具的单次聊天，不保存连续会话 |
| `fetch <URL> [--max-chars N]` | 获取 URL 的响应文本，输出前 N 个字符 |
| `read-config <PATH>` | 加载并校验配置，显示模型等信息及密钥是否设置，不输出密钥值 |
| `help [COMMAND]` | 查看总帮助或指定命令帮助 |
| `--help` / `-h` | 查看帮助；也可放在子命令后 |
| `--version` / `-V` | 查看程序版本，放在顶层使用 |

```sh
cargo run -- --help
cargo run -- help claw
cargo run -- claw --help
cargo run -- --version
```

## LLM 配置

`claw`、`chat` 和 `read-config` 的第一个位置参数是 TOML 配置文件。
文件必须存在；下面是可省略的 `[llm]` 字段及其当前默认值：

```toml
[llm]
base_url = "https://api.openai.com/v1"
model = "gpt-4.1"
temperature = 0.7
# api_key = "..."
```

当配置中的 `api_key` 为空或未设置时，读取环境变量
`AGENT_CLI_LLM_API_KEY`。配置中非空的密钥优先。`claw` 和 `chat`
要求有效的非空密钥；`read-config` 允许密钥为空。temperature 的有效范围
是 0–2，model 不可为空。这些默认值描述本项目配置，不代表模型推荐。

```sh
cargo run -- read-config Config.toml
```

## Claw 单次任务

```sh
cargo run -- claw Config.toml "解释 src/agent.rs 的执行循环" --workspace .
printf '查找会话恢复入口\n' | cargo run -- claw Config.toml --workspace .
```

提供位置参数 `PROMPT` 时使用该参数；否则单次模式读取全部 stdin，直到
EOF。空白输入会报错。单次模式不自动保存会话。

### Claw 参数

| 参数 | 默认值 | 行为与限制 |
| --- | --- | --- |
| `<CONFIG_PATH>` | 必填 | LLM 配置文件路径 |
| `[PROMPT]` | 无 | 单次任务输入，或交互模式的第一条输入 |
| `--workspace <PATH>` | 当前目录 `.` | 已存在的工作目录，启动时规范化真实路径 |
| `--interactive` | 关闭 | 每行一条输入，连续对话并自动存档 |
| `--session-file <PATH>` | 自动路径 | 指定初始新会话存档；必须配合 `--interactive`，父目录存在、目标不存在 |
| `--resume <PATH>` | 不恢复历史 | 恢复并更新已有存档；必须配合 `--interactive`，与 `--session-file` 互斥 |
| `--allow-write` | 关闭 | 允许 `replace_text` 和 `create_file`，包括单文件多处替换 |
| `--allow-exec` | 关闭 | 允许 `run_command`；命令本身可以写文件，不受 `--allow-write` 限制 |
| `--max-iterations <N>` | 无次数上限 | 每条用户请求最多 N 次模型调用，最终回答也计入；N 必须为正整数 |
| `--max-tool-calls <N>` | 无次数上限 | 每条用户请求累计最多 N 次工具调用；整批超限则整批不执行；N 必须为正整数 |
| `--max-context-bytes <N>` | `262144`（256 KiB） | 内部消息 JSON 字节预算，N 必须为正整数；不是 token 数，也不是完整 HTTP 请求大小 |
| `--help` / `-h` | — | 显示 Claw 帮助 |

执行授权示例：

```sh
cargo run -- claw Config.toml "修复问题并运行测试" \
  --workspace /path/to/project --allow-write --allow-exec \
  --max-iterations 12 --max-tool-calls 20
```

执行权限是本次启动的授权。当前没有 OS 沙箱或逐命令审批；工作目录检查
不限制授权命令访问宿主文件或网络。

### Agent 工具与终端命令的区别

这些工具由模型选择调用，不是斜杠命令，也不是 CLI 子命令：

| 工具 | 作用 | 授权 |
| --- | --- | --- |
| `add` | 整数加法 | 默认可用 |
| `list_files` | 列出 workspace 内目录的直接子项 | 默认可用 |
| `read_file` | 有界读取 UTF-8 普通文件，可指定行范围 | 默认可用 |
| `search_code` | 有界递归字面搜索，返回相对路径及行号 | 默认可用 |
| `replace_text` | 精确替换已有文件；支持按顺序执行 1–100 项 edits，全部成功后保存一次 | `--allow-write` |
| `create_file` | 创建文件，不覆盖已有目标，不自动创建父目录 | `--allow-write` |
| `run_command` | 执行程序及参数数组，返回输出、退出码、超时状态和耗时 | `--allow-exec` |

## 交互模式

```sh
cargo run -- claw Config.toml --interactive --workspace /path/to/project
# 也可以先执行一条输入，再进入交互
cargo run -- claw Config.toml "先列出源码文件" --interactive --workspace .
```

| 输入或操作 | 行为 |
| --- | --- |
| 普通非空行 | 作为一条新请求发送，携带当前工作上下文 |
| 空白行 | 跳过，不调用模型 |
| `/sessions` | 列出当前 workspace 的自动存档，不调用模型、不改变当前对话 |
| `/resume <编号>` | 恢复最近一次成功显示列表中的对话，成功后才切换 |
| `/new` | 新建空对话，切换到独立自动存档，保留旧存档 |
| `/exit` | 正常退出，不处理后续输入 |
| EOF | 正常退出；终端通常可在空行按 Ctrl+D |
| Ctrl+C | 等待输入或执行任务时取消并退出，退出码为 130 |

目前识别 `/new`、`/sessions`、`/resume <编号>` 和 `/exit`，会先去掉输入两端空白；它们不会发送给模型。
`/clear`、`/help` 等尚未实现，输入会作为普通消息发送。
命令行中的 `--help` 则由 CLI 解析，不调用模型。

每条请求的迭代次数和工具次数重新计算。`/new` 保留 workspace、上下文
预算、本次写入/执行权限及模型配置；它不撤销已经完成的文件修改或命令操作。

## 自动存档、新建与恢复

交互模式的每轮成功回答后，完整历史与工作上下文保存到：

```text
<workspace>/.agent-cli/sessions/<Unix毫秒时间戳>-<随机标识>.json
```

CLI 显示存档路径。空对话不生成存档文件；正常启动始终是新会话，不自动
恢复最近历史。不同 workspace 使用不同存档目录。

### 显式选择新存档

```sh
cargo run -- claw Config.toml --interactive --workspace /path/to/project \
  --session-file /path/to/archives/task.json
```

显式存档和配置的相对路径按进程当前目录解析，不按 `--workspace` 解析。
显式存档的父目录需要提前存在，已有目标不会被覆盖。

### 恢复旧会话

```sh
cargo run -- claw Config.toml --interactive --workspace /path/to/project \
  --resume /path/to/project/.agent-cli/sessions/<实际文件名>.json
```

将示例中的文件名替换为 CLI 显示的真实路径。恢复时检查存档版本、大小、
workspace 和消息顺序。workspace 必须与存档匹配；当前支持读取 v1–v3，
后续成功保存使用 v3，文件上限为 16 MiB。

恢复历史不会重新执行旧工具，旧会话的写入/执行权限不会继承，需要本次
启动重新传入授权参数。恢复后继续更新原存档；未输入新请求直接退出，
不会重写文件。历史中的文件内容不保证仍与当前磁盘一致。

### 查看会话列表

交互中输入 `/sessions`，显示编号、创建时间（Unix 毫秒）、首条用户输入的
前 80 个字符和文件名。换行及控制字符转义显示。创建时间取自自动存档文件名，
不是最后修改时间；按时间倒序，同时间按文件名排序。

只查看当前 workspace 的 `.agent-cli/sessions/`；目录之外的显式存档不列出。
最多扫描 1000 个目录项、读取 32 MiB、显示 20 个有效存档；达到限制会提示
列表不完整。扫描被截断时，仅对已扫描部分排序，不保证包含整个目录最新的存档。
损坏、超限、workspace 不匹配或不可读的存档跳过并计数；不跟随存档目录或
候选文件的符号链接。列表不显示尚未保存的空对话。

输入 `/resume 1` 可恢复当前列表的第一个存档。编号绑定最近一次成功显示的列表；
新建或保存其他会话不会重新排序该映射，再次执行 `/sessions` 才更新编号。
列表读取失败会清除旧编号，避免误选。没有有效列表、编号为零、越界或参数格式
错误时，只显示用法，不调用模型。

恢复时重新检查文件、workspace 和历史合法性，不重放工具。恢复成功后才替换
当前内存会话和保存目标，继续使用当前模型配置、权限和上下文预算，后续回答更新
所选存档。当前对话已保存的历史保留在原文件中。加载失败（如文件已删除、损坏、
变成符号链接或 workspace 不匹配）时，当前对话保持不变。

查看列表不写存档、不切换对话；列表读取失败会提示错误，仍可继续当前对话。
也可继续使用上文的启动参数 `--resume <路径>` 恢复未列出的显式存档。

```text
>#> /sessions
1. ...
2. ...
>#> /resume 2
已恢复所选对话：...
```

### `/new` 如何处理旧会话

每次接受下一条输入前，上一轮都已经保存成功。`/new` 先分配独立存档路径，
成功后才替换内存会话；旧存档保持不变。即使当前通过 `--session-file` 或
`--resume` 启动，`/new` 后也切换到 workspace 内的新自动存档。

如果新路径分配失败，报告错误并保留当前会话，允许继续输入。
新对话没有成功完成的请求时，不写空存档。

## 上下文与失败行为

- 新请求执行前超限：按完整轮次移除最旧工作上下文并提示移除轮数；完整
  历史仍保存在存档中。被移除的信息不再发送给模型，当前没有自动摘要。
- 新输入自身仍超限：不发送给模型，保留原历史，允许缩短输入或使用 `/new`。
- 工具执行后超限：结束会话，保留上次成功存档；已执行操作不回滚。
- 模型或致命运行错误、取消：结束会话，不把未完成轮次覆盖到存档。
- 可恢复工具错误：作为工具结果返回模型，允许后续继续处理。
- 保存失败：明确报错并结束交互，不继续接受新请求或自动切换会话。
- 存档被外部修改：拒绝继续覆盖；当前检查不提供完整并发隔离。

正常结束的请求会输出最终回答、工具次数和模型 Loop 次数。进度、耗时、
提示和错误输出到 stderr，回答与统计输出到 stdout。普通运行错误退出码
为 1，CLI 参数错误通常为 2。

存档可能包含源码和敏感工具输出。Unix 新文件权限为 0600。此仓库已忽略
`.agent-cli/sessions/`；在其他项目中使用时也应添加对应忽略项。
`search_code` 递归搜索跳过 `.agent-cli`，但显式文件访问和授权命令仍可访问它。

## 其他命令示例

### 单次 Chat

```sh
cargo run -- chat Config.toml "解释这段 Rust 代码"
printf '解释 Result 的作用\n' | cargo run -- chat Config.toml
```

位置参数优先；没有 prompt 时读取全部 stdin。此命令没有工具、交互历史或
自动存档功能。连续编码任务使用 `claw --interactive`。

### Fetch

```sh
cargo run -- fetch https://example.com --max-chars 500
```

`--max-chars` 默认 200，必须为正整数，按字符数限制输出预览；它不是响应
下载大小上限。HTTP 请求超时为 60 秒，非成功状态返回错误。

### 配置检查

```sh
cargo run -- read-config Config.toml
```

显示 base URL、model、temperature，以及密钥状态 `set` 或 `empty`。
它不调用模型，也不测试远端服务连通性。
