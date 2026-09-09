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

最近一次学习中，将 `chat` 中学到的 async HTTP 失败路径迁移到了 `fetch` 命令。`fetch` 现在使用 `reqwest::Client::builder()` 创建 client，并为请求设置 60 秒 timeout；client 构造、`.send().await` 和 `response.text().await` 都添加了明确的错误上下文；非 2xx HTTP status 会通过 `bail!()` 返回错误并触发非 0 exit code。学习中还引入了 `assert_cmd`、`predicates` 和 `wiremock`，为 `fetch` 添加了成功路径和 404 错误路径的 CLI 集成测试。当前 `cargo fmt --check`、`cargo check`、`cargo test` 均通过，测试数量为 17 个单元测试和 2 个集成测试。

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
- 当前 `cargo fmt --check` 通过
- 当前 `cargo check` 通过
- 当前 `cargo test` 通过，17 个单元测试和 2 个集成测试全部通过

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
- stdout / stderr / exit code 测试
- API key 配置边界
- 密钥输出安全边界

当前大多数命令的 `execute()` 仍保持同步并返回 `anyhow::Result<()>`。`wait::execute(seconds)`、`fetch::execute(url)` 和 `chat::execute(config_path, prompt)` 是当前异步命令。`chat` 当前读取 `[llm]` 配置，校验 API key 非空，调用 `openai::send_chat_request()`，并输出 assistant 的文本回复。

## Next Step

下一步围绕 `fetch` 的 CLI 集成测试做复盘和小步扩展：

- Review `tests/fetch_cli.rs` 中 `assert_cmd`、`predicates` 和 `wiremock` 的职责
- 讨论 CLI 集成测试和单元测试的边界
- 讨论是否添加一个小型参数，例如 `fetch --max-chars`，用于练习 CLI 参数设计和测试拆分
- 如果添加 `--max-chars`，优先将 preview 截断逻辑抽成小的纯函数，再分别添加单元测试和集成测试

优先学习目标仍然是 CLI 用户契约、确定性测试、错误输出边界和 async HTTP 行为，而不是继续扩展 Agent 功能。

## Architecture Notes

当前结构：

```text
.
├── src/
│   ├── cli.rs
│   ├── config.rs
│   ├── main.rs
│   ├── openai.rs
│   └── commands/
│       ├── chat.rs
│       ├── divide.rs
│       ├── echo.rs
│       ├── fetch.rs
│       ├── hello.rs
│       ├── read_config.rs
│       ├── repeat.rs
│       ├── sum.rs
│       ├── wait.rs
│       ├── version.rs
│       └── mod.rs
└── tests/
    └── fetch_cli.rs
```

当前职责划分：

- `src/cli.rs`：定义 CLI 结构和子命令，不写业务逻辑
- `src/commands/`：每个命令一个文件，负责具体命令行为
- `src/commands/mod.rs`：统一导出命令模块
- `src/config.rs`：定义配置模型、默认值、业务校验和 `load_config(path)`
- `src/openai.rs`：当前负责 OpenAI-compatible Chat Completions 请求构造、HTTP 请求、HTTP status 判断和响应解析
- `src/main.rs`：`run()` 负责 `Cli::parse()`、`match Commands` 和命令分发；`main()` 负责启动 Tokio runtime、统一错误输出和失败退出码
- `tests/fetch_cli.rs`：使用 `assert_cmd` 运行真实 CLI binary，使用 `wiremock` 提供确定性的本地 HTTP 响应，验证 `fetch` 的 stdout、stderr 和 exit code

配置格式当前为：

```toml
[llm]
base_url = "https://api.deepseek.com"
api_key = "..."
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

- `src/openai.rs` 是否应重命名为 `src/llm.rs` 或 `src/llm/openai_compatible.rs`？
- `send_chat_request()` 是否应该继续在函数内创建新的 `reqwest::Client`，还是等 Agent runtime 阶段再引入 client 复用？
- `fetch` 是否需要支持 `--max-chars` 控制 preview 长度？
- 非 2xx status 是否只输出 status 足够，还是需要后续通过 `--verbose` 暴露 provider 错误 body？
- `chat` 命令是否应支持从环境变量读取 API key？
- `chat` 命令是否应支持 stdin 输入 prompt？
- 是否需要继续为更多命令增加集成测试？
- 是否需要将 `parse_chat_response()` 暴露为更明确的模块边界，还是保持私有函数？
- 是否需要将 LLM provider 配置与通用 CLI 配置进一步分层？

## Technical Debt

- `PROJECT_STATUS.md` 实际位于 `journal/PROJECT_STATUS.md`，不是仓库根目录
- 当前日志文件命名存在 `2026-7-13.md` 这类非标准格式，后续建议统一为 `YYYY-MM-DD.md`
- 旧日志文件 `2026-7-14.md` 与标准命名 `2026-07-14.md` 同时存在，后续需要决定是否迁移或保留
- `Config.toml` 当前是本地运行配置，需要确认是否应改为示例配置或从 Git 中移除真实 key
- 当前只有 `fetch` 有 CLI 集成测试，其他命令暂未覆盖
- `chat` 当前没有端到端自动化测试
- `src/openai.rs` 名称和“支持所有 OpenAI-compatible provider”的目标不完全一致
- 当前没有 `--verbose` 或日志系统，调试 provider 错误 body 不方便

## Next TODO

- [ ] Review `assert_cmd` 和 `wiremock` 测试结构
- [ ] 讨论 `fetch --max-chars` 是否适合作为下一个小功能
- [ ] 讨论 preview 截断逻辑是否应抽成纯函数
- [ ] 练习单元测试和 CLI 集成测试的职责拆分
