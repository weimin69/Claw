# PROJECT STATUS

## Current Stage

当前项目处于 Rust CLI 基础架构学习阶段。

目标是通过一个小型 Agent CLI 项目，逐步掌握：

- Rust 基础语法
- CLI 工程结构
- 模块系统
- 命令分发
- 后续 Agent 能力所需的配置、错误处理、异步请求与 API 调用

当前重点不是快速实现 Agent，而是先建立清晰、可扩展的 CLI 架构，并逐步完善错误处理、配置处理模型、模块边界、基础测试和异步执行入口。项目已经完成 `run()` / `main()` 职责拆分，将命令错误模型迁移到 `anyhow::Result<()>`，并通过 `read-config` 命令练习了文件读取、TOML 解析、错误上下文、默认值和业务校验。

最近一次学习中，继续复习了 `fetch` 的 async HTTP 调用链，并明确区分网络错误、HTTP 状态码和 body 读取错误。`fetch <url>` 已新增 `response.status()` 检查：当 HTTP status 不是成功状态码时，只打印 `status` 并正常返回，不继续读取 body。项目也开始为后续 OpenAI API 调用准备配置模型，新增 `OpenAiConfig`，并在 `Config` 中加入 `openai: Option<OpenAiConfig>`；`read-config` 只输出 OpenAI API key 是否设置，不打印真实 key。当前 `cargo fmt --check`、`cargo check`、`cargo test` 均通过，测试数量仍为 10 个。

## Completed

- 创建 Cargo Rust 项目
- 初始化 GitHub 仓库
- 集成 `clap`
- 使用 `Parser` 定义 CLI 入口
- 使用 `Subcommand` 定义子命令
- 实现 `hello` 命令
- 实现 `version` 命令
- 实现 `echo` 命令
- 为 `echo` 增加必填参数约束
- 为 `echo` 增加 `--upper` flag
- 将 `echo::execute` 调整为接收 `&[String]`
- 使用 `cargo fmt` 统一代码格式
- 实现 `divide` 命令
- 在 `divide` 中处理除以 0 的基础错误输出
- 将 CLI 定义拆分到 `src/cli.rs`
- 将命令实现拆分到 `src/commands/`
- 为每个命令建立独立模块
- 使用 `src/commands/mod.rs` 统一导出命令模块
- 保持 `src/main.rs` 只负责解析参数和命令分发
- 初步理解 Rust 模块系统
- 初步理解 `Vec<String>` 在 CLI 参数中的作用
- 初步理解所有权移动
- 初步理解借用和 slice 参数
- 初步理解 CLI 参数校验和业务校验的边界
- 将 `divide::execute()` 改为返回 `Result<(), String>`
- 将 `echo::execute()`、`hello::execute()`、`version::execute()` 统一改为返回 `Result<(), String>`
- 初步理解 `Ok(())` 和 `Err(String)`
- 初步理解 `if let Err(error)` 的错误处理方式
- 初步理解 CLI 失败时应返回非 0 exit code
- 初步理解 `std::process::exit(1)` 的作用和局限
- 将 `main()` 改为返回 `Result<(), String>`
- 初步使用 `?` 运算符转发错误
- 初步理解“业务函数返回错误，入口层决定如何处理错误”的职责边界
- 实现 `sum` 命令
- 在 `sum::execute()` 中使用 `&[f64]` 只读借用参数
- 验证 `sum` 正常输入、空参数和非法数字参数的行为
- 初步理解 `main()`、`Cli::parse()`、`match` 和命令 `execute()` 之间的调用链
- 初步讨论 `run() -> Result<(), String>` 作为业务入口函数的作用
- 将主要业务入口拆分到 `run() -> Result<(), String>`
- 让 `main()` 调用 `run()`，并使用 `if let Err(error)` 统一处理错误
- 使用 `eprintln!` 将业务错误输出到 `stderr`
- 使用 `std::process::exit(1)` 保持业务错误时返回非 0 exit code
- 初步理解 `stdout` 和 `stderr` 的区别
- 初步理解 `>` 重定向 `stdout`，`2>` 重定向 `stderr`
- 进一步理解 `?` 运算符遇到 `Ok` 会继续执行，遇到 `Err` 会提前返回当前函数
- 修正 `sum` 命令错误文案中的拼写问题
- 实现 `repeat` 命令
- 为 `repeat` 增加 `--times` named argument
- 为 `repeat` 的 `words` 参数增加必填约束
- 在 `repeat::execute()` 中校验 `times == 0` 的业务错误
- 验证 `repeat` 正常输入、业务错误和缺少参数时的行为
- 讨论 `String` 错误模型的优点和局限
- 初步理解 `anyhow` 适合 CLI 应用层错误处理
- 初步理解 `anyhow` 不一定适合库公共 API
- 添加 `anyhow` 依赖
- 将 `run()` 改为返回 `anyhow::Result<()>`
- 将所有命令的 `execute()` 改为返回 `anyhow::Result<()>`
- 使用 `bail!` 表达业务错误
- 验证 `anyhow` 迁移后的正常路径、业务错误路径和 Clap 错误路径
- 实现 `read-config` 命令
- 为 `read-config` 增加路径参数
- 在 `read_config::execute(path: &str)` 中使用只读字符串借用
- 使用 `.context()` 给文件读取失败补充错误上下文
- 初步理解 `with_context()` 适合动态错误上下文
- 验证 `read-config config.toml` 和 `read-config Cargo.toml` 的成功路径
- 验证 `read-config missing.toml` 的失败路径
- 进一步理解相对路径基于程序启动时的当前工作目录
- 修正 `read_config.rs` 的 `with_context()`，让读取失败错误包含具体路径
- 对比理解 `?`、`bail!`、`.context()` 和 `with_context()` 的使用场景
- 初步使用 `struct` 表达配置数据
- 初步理解 `#[derive(Debug)]` 的调试输出用途
- 添加 `serde` 和 `toml` 依赖
- 使用 `serde::Deserialize` 让 `Config` 支持反序列化
- 使用 `toml::from_str()` 将 TOML 字符串解析成 `Config`
- 将 `read-config` 输出调整为读取 `config.model` 和 `config.temperature`
- 验证 `cargo check` 无 warning
- 练习 TOML 字段类型错误、缺少字段和语法错误的失败路径
- 初步理解错误链 error chain
- 使用 `error.chain().skip(1)` 打印底层错误原因
- 验证 `cargo fmt --check` 通过
- 验证 `cargo check` 通过
- 将错误链输出调整为 `caused by:` 分组加编号的格式
- 初步理解 `collect()` 将 iterator 收集为 `Vec`
- 初步理解 `enumerate()` 为迭代器元素生成编号
- 初步理解 `Vec` 不能直接用 `{}` 打印，因为它没有实现 `Display`
- 初步理解 `Option<T>` 用于表达值可能存在或不存在
- 将 `Config.temperature` 从 `f64` 改为 `Option<f64>`
- 使用 `match` 处理 `Some(temperature)` 和 `None`
- 验证缺少 `temperature` 时 `read-config` 输出 `temperature: not set`
- 对比 `Option<T>`、`unwrap_or(default)` 和 `#[serde(default)]` 的适用边界
- 将 `Config.model` 设计为缺失时默认使用 `gpt-4.1`
- 将 `Config.temperature` 设计为缺失时默认使用 `0.7`
- 使用 `#[serde(default = "...")]` 为配置字段提供默认值
- 删除 `temperature` 的 `Option<f64>` 和 `match` 输出逻辑
- 初步理解配置默认值属于配置模型语义时，应放在反序列化层处理
- 为 `Config` 添加 `validate()` 方法
- 初步理解 `impl Config` 方法块
- 初步理解 `&self` 表示方法只读借用当前实例
- 使用 `bail!` 在配置业务校验失败时返回错误
- 校验 `model` 是否属于支持列表
- 校验 `temperature` 是否在 `0.0..=2.0`
- 抽出 `SUPPORTED_MODELS` 常量
- 使用 `contains()` 判断模型是否受支持
- 使用 `String::as_str()` 将 `String` 作为 `&str` 参与匹配
- 使用 `join(", ")` 改进 unsupported model 错误信息
- 验证合法配置、非法模型、非法 `temperature` 的行为
- 验证 `cargo fmt --check` 通过
- 验证 `cargo check` 通过
- 复习配置处理四步：读取文件、解析 TOML、应用默认值、业务校验
- 讨论命令模块和通用配置模块的职责边界
- 初步理解 Rust 模块可见性 `pub`
- 新增 `src/config.rs`
- 将 `Config`、默认值函数、`SUPPORTED_MODELS` 和 `Config::validate()` 移动到配置模块
- 新增 `config::load_config(path) -> anyhow::Result<Config>`
- 在 `src/main.rs` 中注册 `mod config;`
- 将 `read-config` 命令简化为调用 `crate::config::load_config(path)` 并打印结果
- 理解多个命令应该共享底层能力，而不是互相调用 `execute()`
- 验证拆分后 `cargo fmt --check` 通过
- 验证拆分后 `cargo check` 通过
- 验证 `cargo run -- read-config Cargo.toml` 行为正常
- 初步理解 Rust 单元测试结构：`#[cfg(test)]`、`mod tests`、`#[test]` 和 `use super::*`
- 为 `Config` 默认值行为添加单元测试
- 为非法 `model` 添加业务校验测试
- 为非法 `temperature` 添加业务校验测试
- 为 `temperature` 字段类型错误添加 TOML 解析失败测试
- 初步理解测试应按错误发生层次编写：解析错误测反序列化，业务错误测 `validate()`
- 使用 `tempfile::NamedTempFile` 为配置文件读取测试创建临时文件
- 添加 `tempfile` dev-dependency
- 为 `config::load_config(path)` 添加成功路径测试
- 测试 `load_config()` 缺少字段时应用默认值
- 测试 `load_config()` 遇到非法模型时返回业务错误
- 测试 `load_config()` 遇到非法 `temperature` 时返回业务错误
- 测试 `load_config()` 遇到 TOML 解析失败时包含解析上下文
- 测试 `load_config()` 遇到文件读取失败时包含路径上下文
- 验证 `cargo test` 通过，当前 10 个测试全部通过
- 复习配置模块测试分层：文件系统层、TOML 解析层、serde 默认值层、业务校验层
- 初步理解 async Rust 的项目动机：HTTP 请求、流式响应、工具执行和超时控制都可能等待外部资源
- 添加 `tokio` 依赖，并启用 `macros`、`rt-multi-thread` 和 `time` features
- 将 `main()` 改为 `#[tokio::main] async fn main()`
- 将 `run()` 改为 `async fn run() -> anyhow::Result<()>`
- 初步理解 async 函数调用返回 `Future`
- 初步理解 `.await` 等待 `Future` 完成，`?` 处理完成后的 `Result`
- 实现 `wait <seconds>` 命令
- 在 `wait::execute(seconds)` 中使用 `tokio::time::sleep(std::time::Duration::from_secs(seconds)).await`
- 为 `wait` 命令设计业务校验：`seconds` 必须在 `1..=60`
- 验证 `cargo run -- wait 1` 正常路径
- 验证 `cargo run -- wait 0` 业务错误路径
- 复习 `cargo run -- wait 1` 从 `main()` 到程序退出的完整 async 控制流
- 理解 `#[tokio::main]` 通过宏生成 runtime 启动代码
- 区分 `run().await`、`commands::wait::execute(seconds).await?`、`.await` 和 `?` 的职责
- 理解 `.await` 先等待 `Future` 完成，`?` 再处理完成后的 `Result`
- 理解同步命令和异步命令可以在同一个 CLI 中共存
- 清理 `Cargo.toml` 中 `tokio` 依赖附近的多余空行
- 实现 `fetch <url>` 命令骨架
- 在 `src/cli.rs` 中为 `Commands` 增加 `Fetch { url: String }`
- 新增 `src/commands/fetch.rs`
- 在 `src/commands/mod.rs` 中导出 `fetch` 模块
- 在 `src/main.rs` 中增加 `Fetch` 命令分发，并调用 `commands::fetch::execute(&url).await?`
- 添加 `reqwest` 依赖
- 使用 `reqwest::get(url).await?` 发起最小 HTTP GET 请求
- 使用 `response.text().await?` 读取响应 body
- 使用 `text.chars().take(200).collect()` 安全截取前 200 个字符
- 初步理解 HTTP 请求通常至少包含“等待响应”和“读取 body”两个异步阶段
- 初步理解网络错误、HTTP 状态码错误和 body 读取错误不是同一个层次的问题
- 验证 `cargo run -- fetch https://example.com` 在用户本机可以正常打印 HTML 预览
- 验证 `cargo fmt --check` 通过
- 验证 `cargo check` 通过
- 验证 `cargo test` 通过，当前 10 个测试全部通过
- 复习 `fetch` 命令从 `main()` 到程序退出的完整 async HTTP 控制流
- 进一步理解 `.await` 先等待 `Future` 完成，`?` 再处理完成后的 `Result`
- 学习 `response.status()` 读取 HTTP 状态码
- 学习 `status.is_success()` 判断 HTTP status 是否为成功状态
- 将 `fetch` 调整为遇到非成功 HTTP status 时打印状态码并正常返回
- 对比理解 `response.status()` 和 `error_for_status()` 的适用边界
- 初步讨论 OpenAI API 请求需要 URL、POST、Authorization header、JSON body 和 JSON response
- 初步讨论 `chat` 命令和 OpenAI API 客户端模块的职责边界
- 初步理解当前不需要过早引入 `trait LlmClient`
- 初步讨论 `chat` 第一版 prompt 使用位置参数的设计
- 初步讨论 API key 来源：环境变量、配置文件和命令参数的安全性与使用体验
- 新增 `OpenAiConfig`
- 将 `Config` 扩展为包含 `openai: Option<OpenAiConfig>`
- 理解 `Some(value)` 表示有值，`None` 表示没有值
- 理解 `Some(_)` 表示只关心有值，不关心具体内容
- 使用 `match &config.openai` 检查 OpenAI 配置是否存在
- 将 `read-config` 调整为只输出 OpenAI API key 是否设置，不泄露真实 key
- 通过修复编译错误，进一步理解 `Ok(())`、`Result<()>`、import 和函数返回值表达式的关系
- 验证 `cargo fmt --check` 通过
- 验证 `cargo check` 通过
- 验证 `cargo test` 通过，当前 10 个测试全部通过

## In Progress

继续围绕配置读取、配置解析、错误输出边界、配置字段设计、配置业务校验、模块边界、基础测试、async Rust 入门和 HTTP 请求基础推进：

- `serde::Deserialize`
- `toml::from_str()`
- `#[serde(default = "...")]`
- `#[cfg(test)]`
- `#[test]`
- `assert_eq!`
- `assert!`
- `tempfile::NamedTempFile`
- dev-dependencies
- 配置文件格式错误
- 文件读取错误上下文
- 解析错误上下文
- `anyhow::Error::chain()`
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
- `tokio::time::sleep`
- 同步命令和异步命令在同一个 CLI 中共存
- `reqwest`
- HTTP GET
- `Response`
- `response.text().await?`
- 网络错误
- HTTP 状态码
- `response.status()`
- `status.is_success()`
- `error_for_status()`
- body 读取错误
- 字符串字符级截取
- `Option<OpenAiConfig>`
- `Some`
- `None`
- `match` guard
- API key 配置边界
- 密钥输出安全边界

当前大多数命令的 `execute()` 仍保持同步并返回 `anyhow::Result<()>`。`wait::execute(seconds)` 和 `fetch::execute(url)` 是当前两个异步命令，分别用于练习本地 timer 等待和真实 HTTP 请求等待。`fetch` 当前会先检查 HTTP status：成功状态码继续读取 body 并打印 preview，非成功状态码只打印 `status` 并返回 `Ok(())`。`run()` 当前负责解析 CLI、匹配子命令、调用同步命令或在异步命令分支中 `.await`，并使用 `?` 转发业务错误；`main()` 通过 Tokio runtime 驱动 `run().await`，统一打印 `error: ...` 和错误链，然后返回失败退出码。

## Next Step

下一步建议先为新增 OpenAI 配置模型补充最小测试，避免配置结构变化只停留在手动验证：

- 测试缺少 `[openai]` 时 `config.openai` 为 `None`
- 测试存在 `[openai] api_key` 时 `config.openai` 为 `Some`
- 确认测试中不打印、不暴露真实 API key

完成这些测试后，再设计第一版 `chat` 命令：

- 位置参数 prompt
- 配置文件读取路径
- OpenAI API key 缺失时的错误信息
- `src/openai.rs` 的最小函数边界
- OpenAI API 状态码处理策略

## Architecture Notes

当前项目结构：

```text
src
├── commands
│   ├── echo.rs
│   ├── divide.rs
│   ├── fetch.rs
│   ├── hello.rs
│   ├── read_config.rs
│   ├── repeat.rs
│   ├── sum.rs
│   ├── wait.rs
│   ├── version.rs
│   └── mod.rs
├── cli.rs
├── config.rs
└── main.rs
```

当前职责划分：

- `src/cli.rs`：定义 CLI 结构和子命令，不写业务逻辑
- `src/commands/`：每个命令一个文件，负责具体业务逻辑；多数同步命令通过 `anyhow::Result<()>` 返回执行结果，`wait` 和 `fetch` 是当前异步命令
- `src/commands/mod.rs`：统一导出命令模块
- `src/config.rs`：定义配置模型、默认值、支持模型列表、业务校验和 `load_config(path)`
- `src/main.rs`：`run()` 负责 `Cli::parse()`、`match Commands`、调用对应 `execute()`，并通过 `?` 转发错误；异步命令分支使用 `.await?`；`main()` 使用 `#[tokio::main]` 启动 runtime，调用 `run().await`，统一打印错误、打印错误链和设置失败退出码
- `read-config` 当前用于配置读取和解析练习，负责调用 `config::load_config(path)` 并输出配置字段
- `Config.model` 当前通过 `#[serde(default = "default_model")]` 缺失时默认使用 `gpt-4.1`
- `Config.temperature` 当前通过 `#[serde(default = "default_temperature")]` 缺失时默认使用 `0.7`
- `Config.openai` 当前为 `Option<OpenAiConfig>`，用于表达配置文件中可能存在或缺少 `[openai]` 配置
- `OpenAiConfig.api_key` 当前只用于判断 key 是否设置；`read-config` 不打印真实 key
- `Config::validate()` 当前校验模型支持列表和 `temperature` 范围
- `SUPPORTED_MODELS` 当前定义了允许的模型列表：`gpt-4.1`、`gpt-4.1-mini`
- `Config` 和需要跨模块读取的字段当前使用 `pub` 暴露给命令层
- `src/config.rs` 当前包含单元测试模块，测试配置默认值、业务校验失败、TOML 字段类型错误、真实文件加载和错误上下文
- `tempfile` 当前只作为 dev-dependency，用于配置模块测试
- `wait <seconds>` 当前用于 async 入门练习，业务规则为 `seconds` 必须在 `1..=60`
- `tokio` 当前作为普通 dependency，用于 runtime、宏和定时器能力
- `fetch <url>` 当前用于 HTTP 请求基础练习，使用 `reqwest::get` 请求 URL；成功状态码读取文本 body 并打印前 200 个字符，非成功状态码只打印 `status`
- `reqwest` 当前作为普通 dependency，用于异步 HTTP 客户端能力

新增命令时应遵循：

1. 在 `src/cli.rs` 中定义命令参数
2. 在 `src/commands/` 下新增命令模块
3. 在 `src/commands/mod.rs` 中导出模块
4. 在 `src/main.rs` 中添加命令分发
5. 在命令模块中提供返回 `anyhow::Result<()>` 的 `execute()` 函数；如果命令需要 `.await`，则将该命令的 `execute()` 设计为 `async fn`

## Open Questions

- TOML 解析失败时，错误信息应该暴露多少底层细节？
- `read-config` 是调试命令，还是未来真实 Agent 配置加载流程的一部分？
- 支持模型列表后续应该硬编码、放入配置，还是由 API 能力发现？
- 后续命令越来越多时，是否需要改进 `main.rs` 中的分发方式？
- 后续是否需要为命令执行结果和错误输出增加自动化测试？
- `Config` 字段长期保持 `pub`，还是后续改为访问器方法？
- 当前配置模块测试是否已经足够支撑进入 async 和 HTTP 客户端学习？
- `wait` 命令是否需要 CLI 行为测试，还是暂时作为手动验证的 async 练习命令？
- `run()` 变成 async 后，后续是否应该保持同步命令原样，还是逐步统一命令接口？
- `fetch` 是否需要超时控制，避免网络请求长时间挂起？
- `read-config` 是否应该长期展示密钥设置状态，还是只作为当前学习阶段的调试输出？
- 后续 `chat` 命令应该如何指定配置文件路径？
- 后续 `chat` 命令应优先读取环境变量 `OPENAI_API_KEY`，还是配置文件 `[openai].api_key`？
- 后续 OpenAI API 请求模块应命名为 `src/openai.rs`，还是提前放入 `src/llm/openai.rs`？

## Technical Debt

- 当前命令没有测试
- 项目根目录存在 `config.toml`，需要决定它是示例配置、测试配置，还是临时文件
- 当前项目根目录没有 `PROJECT_STATUS.md`，实际状态文件位于 `journal/PROJECT_STATUS.md`
- 当前日志文件命名存在 `2026-7-13.md`、`2026-7-14.md`，后续建议统一为 `YYYY-MM-DD.md`
- 旧日志文件 `2026-7-14.md` 与标准命名 `2026-07-14.md` 同时存在，后续需要决定是否迁移或保留
- 配置测试数据缩进可以继续整理，提高可读性
- `src/config.rs` 中部分 raw string TOML 测试数据仍有缩进残留，可下次继续整理
- `fetch` 当前没有超时控制
- `fetch` 当前没有自动化测试
- `Config.openai` 当前没有专门的单元测试
- `read-config` 的 OpenAI key 状态输出当前没有自动化测试

## Next TODO

- [ ] 为缺少 `[openai]` 的配置添加测试
- [ ] 为存在 `[openai] api_key` 的配置添加测试
- [ ] 设计第一版 `chat` 命令的 CLI 参数
- [ ] 决定 `chat` 命令从哪里读取配置文件
- [ ] 设计 `src/openai.rs` 的最小函数边界
- [ ] 设计 OpenAI API key 缺失时的错误信息
