# PROJECT STATUS

## Current Stage

当前项目处于 Rust CLI 基础架构、Async Rust 入门和 LLM API 集成的交界阶段。

目标仍然是通过一个小型 Agent CLI 项目，逐步掌握：

- Rust 基础语法
- CLI 工程结构
- 模块系统
- 命令分发
- 错误处理
- 配置模型
- 异步 HTTP 请求
- OpenAI-compatible Chat Completions API 调用
- 后续 Agent Runtime 所需的基础边界

项目已经完成基础多命令 CLI、`run()` / `main()` 职责拆分、`anyhow::Result<()>` 错误模型、配置读取/解析/默认值/校验、基础单元测试、`tokio` 异步入口、`fetch` HTTP GET 练习、第一版 `chat` 命令，以及第一批 CLI 集成测试。

最近一次学习中，深化了 Tokio Task 的错误边界，将 `parallel-wait` Task 调整为返回业务 `Result`，并明确区分外层 `JoinError` 与内层 `anyhow::Error`。当前选择 wait-all 作为失败策略，抽取私有 `wait_for_both()`，保证两个 Task 均完成等待后再传播错误。新增确定性测试验证第一个 Task panic 或返回业务错误时，仍会等待第二个 Task 完成。当前格式、编译、5 个 `parallel-wait` 单元测试和 2 个 CLI 集成测试均通过。

## Completed

- 创建 Cargo Rust 项目
- 初始化 GitHub 仓库
- 集成 `clap`
- 使用 `Parser` 定义 CLI 入口
- 使用 `Subcommand` 定义子命令
- 实现 `hello`、`version`、`echo`、`divide`、`sum`、`repeat`、`read-config`、`wait`、`fetch` 和 `chat` 命令
- 将 CLI 定义拆分到 `src/cli.rs`
- 将命令实现拆分到 `src/commands/`
- 使用 `src/commands/mod.rs` 统一导出命令模块
- 将主要业务入口拆分到 `run()`
- 使用 `main()` 统一处理错误输出和失败退出码
- 将命令错误模型迁移到 `anyhow::Result<()>`
- 使用 `?` 转发错误
- 使用 `bail!()` 表达业务错误
- 使用 `.context()` / `.with_context()` 补充错误上下文
- 初步理解 stdout / stderr 和非 0 exit code
- 使用 `serde::Deserialize` 解析 TOML 配置
- 使用 `#[serde(default = "...")]` 和 `Default` 处理配置默认值
- 新增 `src/config.rs`，集中管理配置模型、默认值、业务校验和 `load_config(path)`
- 将配置模型调整为 `[llm]`
- 新增 `LlmConfig`
- 支持 `llm.base_url`
- 支持 `llm.api_key`
- 支持 `llm.model`
- 支持 `llm.temperature`
- 默认 `base_url` 为 `https://api.openai.com/v1`
- 默认 `model` 为 `gpt-4.1`
- 默认 `temperature` 为 `0.7`
- 校验 `model` 不能为空
- 校验 `temperature` 必须位于 `0.0..=2.0`
- `read-config` 输出 LLM 配置状态，且不打印真实 API key
- 为配置模块添加单元测试
- 使用 `tempfile::NamedTempFile` 测试真实配置文件加载
- 添加 `tokio` 依赖并启用 async runtime
- 将 `main()` 改为 `#[tokio::main] async fn main()`
- 将 `run()` 改为 `async fn run() -> anyhow::Result<()>`
- 实现 `wait <seconds>` 命令练习 `tokio::time::sleep`
- 添加 `reqwest`
- 实现 `fetch <url>` HTTP GET 练习
- 区分网络错误、HTTP status 错误和 body 读取错误
- `fetch` 使用 `reqwest::Client::builder()` 创建 HTTP client
- 为 `fetch` HTTP 请求设置 60 秒 timeout
- 为 `fetch` client 构造失败添加错误上下文
- 为 `fetch` `.send().await` 添加请求发送失败上下文
- 为 `fetch` `response.text().await` 添加 body 读取失败上下文
- `fetch` 非 2xx HTTP status 通过 `bail!()` 返回错误
- 理解 timeout 发生在 `.send().await` 阶段，而不是 client 构造阶段
- 添加 `serde_json`
- 添加 `reqwest` 的 `json` feature
- 使用 `serde::Serialize` 构造 Chat Completions 请求体
- 设计 Chat Completions 请求结构：`model`、`temperature`、`messages`、`stream`
- 实现 `chat <config_path> <prompt>` 命令
- 使用 `reqwest::Client::builder()` 创建 HTTP client
- 为 `chat` HTTP 请求设置 60 秒 timeout
- 使用 `.post(...).bearer_auth(...).json(...).send().await` 发送请求
- 为 HTTP client 构造失败添加错误上下文
- 为 `.send().await` 添加请求发送失败上下文
- 为 `response.text().await` 添加 body 读取失败上下文
- 使用 `serde::Deserialize` 定义 Chat Completions 响应结构
- 解析 `choices[0].message.content`
- 抽出 `parse_chat_response(text: &str) -> Result<String>`
- 抽出 `ensure_success_status(status: reqwest::StatusCode) -> Result<()>`
- 非 2xx HTTP status 返回错误，不再静默返回空字符串
- 默认错误输出不再打印 provider 原始错误 body，避免泄露密钥相关信息
- 理解 `response.text().await` 会消费 `response`，因此需要先保存 `status`
- 为请求构造添加单元测试
- 为响应解析正常路径添加单元测试
- 为空 `choices` 添加错误路径测试
- 为 `content: null` 添加错误路径测试
- 为非法 JSON 添加错误上下文测试
- 为成功和失败 HTTP status 判断添加单元测试
- 添加 `assert_cmd`、`predicates` 和 `wiremock` 作为 dev-dependencies
- 新增 `tests/fetch_cli.rs` CLI 集成测试文件
- 为 `fetch` 成功响应添加 CLI 集成测试
- 为 `fetch` 非 2xx status 添加 CLI 集成测试
- 为 `fetch` 添加 `--max-chars <MAX_CHARS>` 参数
- `fetch --max-chars` 默认值为 `200`
- 将 `fetch` response preview 截断逻辑抽成 `preview_text(text, max_chars)` 纯函数
- 使用 `.chars()` 按字符数截断 preview，避免按字节破坏 UTF-8 文本
- 为 `preview_text` 添加单元测试，覆盖英文、中文和最大长度超过文本长度的情况
- 为 `fetch --max-chars` 添加 CLI 集成测试
- 使用 clap 自定义 parser `parse_positive_usize` 校验 `--max-chars` 必须大于 0
- 为 `fetch --max-chars 0` 添加 CLI 参数错误集成测试
- 新增 `tests/chat_cli.rs` CLI 集成测试文件
- 使用 `wiremock` 为 `chat` 命令提供确定性的本地 mock HTTP server
- 使用 `tempfile::NamedTempFile` 在 `chat` CLI 集成测试中创建临时 `[llm]` 配置
- 为 `chat` happy path 添加 CLI 集成测试，验证 assistant content 输出到 stdout
- 为缺少 `llm.api_key` 添加 CLI 错误路径测试
- 为 `chat` provider 非 2xx status 添加 CLI 错误路径测试
- 在 `chat` happy path CLI 集成测试中使用 `body_json` 验证请求体
- 理解 `wiremock` path 必须匹配真实请求路径，否则测试会变成 mock 未命中
- 理解 provider 错误路径测试应先保证配置合法，避免提前停在配置校验阶段
- 理解 `chat_cli.rs` 集成测试和 `openai.rs` 单元测试的分工
- 理解 `chat.rs`、`config.rs`、`openai.rs` 的职责边界
- 理解 `wiremock` 让 `chat` CLI 集成测试不依赖真实网络、真实 API key、provider 可用性、余额、限流或模型行为变化
- 理解 provider 错误路径断言 stdout 为空是在保护 CLI 用户输出契约
- 为 `chat` happy path 添加 `stderr` 为空断言
- 为 `chat` 缺少 `llm.api_key` 错误路径添加 `stdout` 为空断言
- 为 `chat` provider 非 2xx 错误路径添加 `stdout` 为空断言
- 在 `tests/chat_cli.rs` 中抽取 `write_temp_config(contents)` helper
- 理解测试 helper 应隐藏机械准备步骤，而不是隐藏测试意图
- 理解成功路径和失败路径的 stdout / stderr / exit code 契约
- 理解配置合法性和命令运行前置条件不是同一件事
- 理解空 `model` 属于配置通用非法状态，而空 `api_key` 是 `chat` 命令运行前置条件
- 理解 `Option<String>` 用于表达 provider 响应字段可能缺失或为 `null`
- 初步理解从借用结构中取出 owned `String` 时为什么需要 `.clone()` 或其他所有权处理
- 理解 `mod openai;` 是将 `src/openai.rs` 加入 crate 模块树
- 初步讨论 `reqwest::Client` 当前函数内创建与未来 Agent Runtime 复用的取舍
- 理解自动化测试不应直接调用真实 LLM API
- 理解 `cargo test --test chat_cli` 中测试 target 名不包含 `.rs`
- 理解 `Command::cargo_bin(...)` 的 binary 名应匹配 Cargo package/bin 名
- 复盘 `first_word(input: &str) -> &str` 能返回引用的原因：返回值借用自调用者仍然拥有的数据
- 复盘生命周期标注只描述引用关系，不会延长局部变量生命周期
- 将 `parse_chat_response()` 改为先 `.as_ref()` 借用 `Option<String>`，再 `.clone()` 返回 owned `String`
- 理解 `parse_chat_response()` 中非法 JSON、空 `choices`、缺失 `content` 是三个不同错误边界
- 为 `chat` happy path CLI 集成测试添加 `Authorization: Bearer test-key` header 断言
- 支持通过 `AGENT_CLI_LLM_API_KEY` 为 LLM API key 提供环境变量 fallback
- 明确 TOML 中显式 `llm.api_key` 优先于环境变量 fallback
- 明确空字符串或全空格环境变量不应写入 `llm.api_key`
- 将真实环境变量读取和配置合并规则拆开，避免单元测试污染进程全局环境
- 为配置合并规则添加单元测试：缺失 key 时使用 env、已有 TOML key 时不覆盖、空白 env key 被忽略
- 为 `chat` 添加 env API key fallback 的 CLI 集成测试
- 理解 `cmd.env(...)` 只影响当前被测 CLI 子进程，避免污染当前测试进程的全局环境变量
- 理解 env fallback 集成测试中配置文件必须省略 `api_key`，否则会退化为普通 happy path
- 为 env fallback 集成测试断言 `Authorization: Bearer env-key`，证明环境变量结果进入真实 HTTP 请求契约
- 复盘 stdout、stderr 和 Authorization header 分别保护用户结果契约、错误/诊断契约和 provider 请求契约
- 复盘当前 `send_chat_request()` 内部创建 `reqwest::Client` 的设计取舍
- 明确当前阶段暂不提前抽象 `LlmClient`，未来 Agent Runtime 多次调用 LLM 时再考虑 client 复用
- 将 `chat` 的命令行 prompt 从 `String` 改为 `Option<String>`
- 支持在命令行 prompt 缺失时从 stdin 读取 prompt
- 明确命令行 prompt 与 stdin 同时存在时，显式命令行参数优先
- 使用 `std::io::Read::read_to_string()` 将 stdin 读取到 owned `String`
- 在输入来源选择后统一校验最终 prompt 不能为空或全为空白
- 理解 stdin 是输入通道，prompt 是业务数据，两者并非同一概念
- 理解 `match` 分支必须产生兼容类型，并使用 `?` 从 `Result<String>` 中取得值或传播错误
- 理解 owned `String` 可以直接移动进请求结构，无需额外 `.to_string()`
- 为 stdin prompt fallback 添加 CLI 集成测试并使用 `body_json` 验证请求体
- 为命令行 prompt 优先于 stdin 添加 CLI 集成测试
- 为仅包含空白的命令行 prompt 添加 CLI 集成测试
- 为仅包含空白的 stdin prompt 添加 CLI 集成测试
- 理解输入来源选择与最终 prompt 业务校验是两个不同步骤
- 理解显式空白命令行参数不会静默回退到 stdin
- 理解异步 I/O 的主要价值是在等待期间允许执行其他任务，而不是让单次 I/O 更快
- 明确当前一次性 `chat` 流程继续同步读取 stdin，未来交互式 Agent Runtime 再评估异步 stdin
- 理解 wiremock 意外返回 404 通常表示请求 matcher 未命中
- 将 `src/openai.rs` 重命名为 `src/openai_compatible.rs`，准确表达多个 Provider 可复用的协议边界
- 明确 Provider 数量增加不等于协议实现数量增加
- 明确当前只有一种协议实现，不创建 `llm/` 目录或 Provider trait
- 明确传入具体 `reqwest::Client` 可以集中配置和复用连接，但不会自动使网络层可 mock
- 明确当一次进程或 Agent run 需要多次调用 LLM 时，再重新评估 Client 生命周期
- 明确默认错误输出不应直接暴露 Provider 原始 body，未来 verbose 输出也需要截断和脱敏
- 区分 `fetch --max-chars` 的终端输出限制与 response body 的资源限制
- 理解 wiremock 返回 404 通常表示请求到达 mock server 但 matcher 未命中
- 明确当前 `LlmConfig` 无需进一步分层，`parse_chat_response()` 保持私有
- 当前 `cargo check` 通过
- 当前 `cargo fmt --check` 通过
- 当前 `cargo test openai_compatible` 通过，7 个协议单元测试全部通过
- 当前 `cargo test config` 通过，13 个配置相关测试全部通过
- 当前 `cargo test --test chat_cli` 通过，8 个 `chat` CLI 集成测试全部通过
- 新增 `parallel-wait <FIRST_SECONDS> <SECOND_SECONDS>` 命令
- 使用两个 `tokio::spawn` Task 并发执行等待操作
- 理解连续 `.await` 不会自动产生并发
- 理解多个 Task 需要先创建再等待，生命周期才能重叠
- 理解 `JoinHandle::await` 和 `JoinError` 的基础边界
- 区分 Task 启动顺序、完成顺序、结果获取顺序和 CLI 输出顺序
- 在 Task 创建前校验等待参数范围，避免失败后仍启动部分任务
- 为 `parallel-wait` 添加成功输出顺序和非法输入 CLI 集成测试
- 为 Tokio 启用 `test-util` feature
- 使用 `#[tokio::test(start_paused = true)]` 和 timeout 确定性验证并发行为
- 理解测试阈值必须能够区分并发实现和顺序回归
- 当前 `cargo test` 通过，共 36 个测试
- 将 `parallel-wait` Task 输出调整为 `anyhow::Result<u64>`
- 理解 `Result<Result<T, E>, JoinError>` 的双层错误模型
- 明确 `bail!` 返回内层业务错误，Task panic 返回外层 `JoinError`
- 理解 `handle.await??` 与分步骤拆解两层 `Result` 的等价关系
- 理解丢弃 `JoinHandle` 默认不会自动取消 Task
- 为 `parallel-wait` 选择 wait-all 失败策略
- 抽取私有 `wait_for_both()` 集中处理等待与错误传播
- 使用虚拟时间验证 panic 后仍等待第二个 Task
- 使用虚拟时间验证业务错误后仍等待第二个 Task
- 当前 `parallel-wait` 5 个单元测试和 2 个 CLI 集成测试通过

## In Progress

继续围绕配置读取、错误输出边界、模块边界、基础测试、async Rust、HTTP 请求基础和 OpenAI-compatible Chat Completions 推进：

- `serde::Deserialize`
- `serde::Serialize`
- `serde_json`
- `toml::from_str()`
- `#[serde(default = "...")]`
- `Default`
- `#[cfg(test)]`
- `#[test]`
- `assert_eq!`
- `assert!`
- `tempfile::NamedTempFile`
- `assert_cmd`
- `predicates`
- `wiremock`
- dev-dependencies
- 配置文件格式错误
- 文件读取错误上下文
- 解析错误上下文
- `anyhow::Error::chain()`
- `bail!`
- `.context()`
- `.with_context()`
- 配置结构体字段设计
- 默认值的放置边界
- 配置业务校验
- 用户输出和调试输出的边界
- 模块可见性 `pub`
- 命令层和配置模块的职责边界
- 配置模块单元测试的覆盖边界
- `tokio`
- `#[tokio::main]`
- `async fn`
- `Future`
- `.await`
- `.await?`
- 同步命令和异步命令在同一个 CLI 中共存
- `reqwest::Client`
- HTTP GET
- HTTP POST
- `Response`
- `response.status()`
- `status.is_success()`
- `response.text().await?`
- `.json(&request)`
- Bearer token authentication
- OpenAI-compatible Chat Completions
- 请求体序列化
- 响应体反序列化
- 确定性单元测试
- CLI 集成测试
- mock HTTP server
- `body_json`
- stdout / stderr / exit code 测试
- LLM CLI 集成测试
- 临时配置文件测试
- 不依赖真实 provider 的确定性测试
- clap 自定义 value parser
- CLI 参数合法性校验
- 函数作为 parser 传给 clap，而不是立即调用
- Rust 函数末尾表达式返回值
- 字符截断和字节长度的区别
- API key 配置边界
- 密钥输出安全边界
- CLI 集成测试与单元测试的分工
- HTTP request body matcher
- `reqwest::Client` 复用边界
- CLI 用户契约
- 测试 helper 抽取边界
- 配置合法性 vs 命令运行前置条件
- provider 协议边界
- `Option<String>`
- owned `String` vs `&str`
- 从借用结构中移动字段的所有权限制
- `std::env::var`
- 环境变量 fallback
- 配置来源优先级
- 外部状态读取与纯配置合并逻辑拆分
- 进程全局环境变量对并发测试的影响
- `assert_cmd::Command::env`
- HTTP client 生命周期
- `reqwest::Client` 复用边界
- Agent Runtime 中 LLM client 的未来边界
- stdin 与命令行参数的输入优先级
- `std::io::Read`
- `stdin()`
- `read_to_string()`
- 输入来源选择后的统一业务校验

当前大多数命令的 `execute()` 仍保持同步并返回 `anyhow::Result<()>`。`wait`、`fetch`、`chat` 和 `parallel-wait` 是当前异步命令。`parallel-wait` 先创建两个返回 `anyhow::Result<u64>` 的 Tokio Task，再通过私有 `wait_for_both()` 等待两个 handle，之后按固定顺序传播 `JoinError` 和业务错误并输出结果。暂停时间测试既验证成功路径并发，也验证 panic 与业务错误路径的 wait-all 行为。

## Next Step

下一步完善 wait-all 的错误契约：

- 分别校验 `first_seconds` 和 `second_seconds`，返回精确参数错误
- 为第一、第二 Task 的 `JoinError` 和业务错误增加明确上下文
- 明确两个 Task 同时失败时当前按固定顺序返回哪个错误
- 讨论是否需要保留多个错误，还是当前阶段只返回一个主错误
- 在理解错误优先级后，再评估显式取消与生产 timeout

继续一次只引入一个主要 Async Rust 概念；暂不引入 channel、通用任务集合、Provider trait 或完整 Agent Loop。

## Architecture Notes

当前结构：

```text
.
├── src/
│   ├── cli.rs
│   ├── config.rs
│   ├── main.rs
│   ├── openai_compatible.rs
│   └── commands/
│       ├── chat.rs
│       ├── divide.rs
│       ├── echo.rs
│       ├── fetch.rs
│       ├── hello.rs
│       ├── parallel_wait.rs
│       ├── read_config.rs
│       ├── repeat.rs
│       ├── sum.rs
│       ├── wait.rs
│       ├── version.rs
│       └── mod.rs
└── tests/
    ├── parallel_wait_cli.rs
    ├── fetch_cli.rs
    └── chat_cli.rs
```

当前职责划分：

- `src/cli.rs`：定义 CLI 结构和子命令，不写业务逻辑
- `src/commands/`：每个命令一个文件，负责具体命令行为；`chat.rs` 也负责选择命令行 prompt 或 stdin，并校验最终 prompt
- `src/commands/mod.rs`：统一导出命令模块
- `src/config.rs`：定义配置模型、默认值、环境变量 fallback、业务校验和 `load_config(path)`
- `src/openai_compatible.rs`：负责 OpenAI-compatible Chat Completions 请求构造、HTTP 请求、HTTP status 判断和响应解析
- `src/main.rs`：`run()` 负责 `Cli::parse()`、`match Commands` 和命令分发；`main()` 负责启动 Tokio runtime、统一错误输出和失败退出码
- `tests/fetch_cli.rs`：使用 `assert_cmd` 运行真实 CLI binary，使用 `wiremock` 提供确定性的本地 HTTP 响应，验证 `fetch` 的 stdout、stderr、exit code 和参数解析行为
- `tests/chat_cli.rs`：使用 `assert_cmd` 运行真实 CLI binary，使用 `wiremock` 提供确定性的本地 OpenAI-compatible HTTP 响应，使用临时配置文件验证 `chat` 的 stdout、stderr、exit code、配置边界、请求体边界、stdin fallback、输入优先级和空白 prompt 错误路径
- `src/commands/parallel_wait.rs`：创建两个返回业务 `Result` 的 Tokio Task，通过私有 `wait_for_both()` 实现 wait-all，按参数顺序传播错误和输出结果，并使用暂停时间测试成功与失败时序
- `tests/parallel_wait_cli.rs`：验证 `parallel-wait` 的成功输出顺序、失败退出码、stdout 和 stderr 契约

配置格式当前为：

```toml
[llm]
base_url = "https://api.deepseek.com"
api_key = "..." # 可省略；为空时回退到 AGENT_CLI_LLM_API_KEY
model = "deepseek-v4-pro"
temperature = 0.7
```

新增命令时应遵循：

1. 在 `src/cli.rs` 中定义命令参数
2. 在 `src/commands/` 下新增命令模块
3. 在 `src/commands/mod.rs` 中导出模块
4. 在 `src/main.rs` 中添加命令分发
5. 在命令模块中提供返回 `anyhow::Result<()>` 的 `execute()` 函数；如果命令需要 `.await`，则将该命令的 `execute()` 设计为 `async fn`

## Open Questions

- `send_chat_request()` 是否应该继续在函数内创建新的 `reqwest::Client`，还是等 Agent runtime 阶段再引入 client 复用？
- `fetch --max-chars` 是否需要增加上限，避免用户意外读取和输出过大的 preview？
- 非 2xx status 是否只输出 status 足够，还是需要后续通过 `--verbose` 暴露 provider 错误 body？
- 当前同步读取 stdin 是否应在未来交互式 Agent Runtime 阶段迁移为 Tokio 异步 I/O？
- 是否需要继续为更多命令增加集成测试？
- 是否需要为 `AGENT_CLI_LLM_API_KEY` 增加用户文档或示例配置说明？
- 后续是否需要显式取消仍在运行的兄弟 Task？
- 两个 wait-all Task 同时失败时，应该只返回固定顺序的第一个错误，还是聚合多个错误？

## Technical Debt

- `PROJECT_STATUS.md` 实际位于 `journal/PROJECT_STATUS.md`，不是仓库根目录
- `tests/chat_cli.rs` 和 `src/openai_compatible.rs` 的测试 fixture 仍有轻微缩进可读性空间，但不影响当前学习主线
- 当前日志文件命名存在 `2026-7-13.md` 这类非标准格式，后续建议统一为 `YYYY-MM-DD.md`
- 旧日志文件 `2026-7-14.md` 与标准命名 `2026-07-14.md` 同时存在，后续需要决定是否迁移或保留
- `Config.toml` 当前是本地运行配置，需要确认是否应改为示例配置或从 Git 中移除真实 key
- 当前主要只有 `fetch`、`chat` 和 `parallel-wait` 有 CLI 集成测试，其他命令暂未覆盖
- `parallel-wait` 当前将两个参数的校验合并处理，错误信息不能指出具体非法参数
- `wait_for_both()` 当前按 first、second 的固定顺序传播错误，多个错误不会被聚合
- `wait_for_both()` 当前未为具体 Task 的 JoinError 和业务错误补充上下文
- 当前没有 `--verbose` 或日志系统，调试 provider 错误 body 不方便

## Next TODO

- [ ] 分别校验 `first_seconds` 与 `second_seconds` 并提供精确错误信息
- [ ] 为两个 Task 的 JoinError 与业务错误补充明确上下文
- [ ] 明确两个 Task 同时失败时的错误优先级与信息保留策略
