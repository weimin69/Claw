# PROJECT STATUS

## Current Stage

当前项目处于 Rust CLI 基础架构学习阶段。

目标是通过一个小型 Agent CLI 项目，逐步掌握：

- Rust 基础语法
- CLI 工程结构
- 模块系统
- 命令分发
- 后续 Agent 能力所需的配置、错误处理、异步请求与 API 调用

当前重点不是快速实现 Agent，而是先建立清晰、可扩展的 CLI 架构，并逐步完善错误处理、配置处理模型、模块边界和基础测试。项目已经完成 `run()` / `main()` 职责拆分，将命令错误模型迁移到 `anyhow::Result<()>`，并通过 `read-config` 命令练习了文件读取、TOML 解析、错误上下文、默认值和业务校验。

最近一次学习中，配置模块已经开始补充单元测试。当前已测试配置默认值、非法模型、非法 `temperature` 和 TOML 字段类型错误；`cargo test` 当前 4 个测试全部通过。下一步重点是继续测试 `config::load_config(path)` 的文件读取、解析上下文和完整配置加载流程。

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
- 验证 `cargo test` 通过，当前 4 个测试全部通过

## In Progress

继续围绕配置读取、配置解析、错误输出边界、配置字段设计、配置业务校验、模块边界和基础测试推进：

- `serde::Deserialize`
- `toml::from_str()`
- `#[serde(default = "...")]`
- `#[cfg(test)]`
- `#[test]`
- `assert_eq!`
- `assert!`
- 配置文件格式错误
- 解析错误上下文
- `anyhow::Error::chain()`
- 配置结构体字段设计
- 默认值的放置边界
- 配置业务校验
- 用户输出和调试输出的边界
- 模块可见性 `pub`
- 命令层和配置模块的职责边界
- 配置模块单元测试的覆盖边界

当前所有命令的 `execute()` 已统一返回 `anyhow::Result<()>`。`run()` 负责解析 CLI、匹配子命令并使用 `?` 转发业务错误；`main()` 负责统一打印 `error: ...`，并在存在底层错误时以 `caused by:` 分组编号格式打印错误链，然后返回失败退出码。`read-config` 已能接收路径参数，并通过 `config::load_config(path)` 读取 TOML 文件、解析为 `Config`、应用默认值、执行业务校验，然后输出 `model` 与 `temperature` 字段。`src/config.rs` 当前已有 4 个单元测试，覆盖默认值、业务校验失败和字段类型解析失败。

## Next Step

下一步建议继续完善配置模块测试：

- 修正测试数据中的 `unknown-model` 拼写
- 为 `config::load_config(path)` 编写基础测试
- 测试真实文件中缺少 `model` 和 `temperature` 时会应用默认值
- 测试 `load_config()` 遇到非法 `model` 会返回业务错误
- 测试 `load_config()` 遇到非法 `temperature` 会返回业务错误
- 测试 TOML 语法错误或字段类型错误会带有解析上下文
- 继续确认 `read-config` 是调试命令，真实配置加载能力由 `config::load_config()` 提供

完成这些理解后，再进入：

- `reqwest`
- `async`
- OpenAI API
- Agent loop

## Architecture Notes

当前项目结构：

```text
src
├── commands
│   ├── echo.rs
│   ├── divide.rs
│   ├── hello.rs
│   ├── read_config.rs
│   ├── repeat.rs
│   ├── sum.rs
│   ├── version.rs
│   └── mod.rs
├── cli.rs
├── config.rs
└── main.rs
```

当前职责划分：

- `src/cli.rs`：定义 CLI 结构和子命令，不写业务逻辑
- `src/commands/`：每个命令一个文件，负责具体业务逻辑，并通过 `anyhow::Result<()>` 返回执行结果
- `src/commands/mod.rs`：统一导出命令模块
- `src/config.rs`：定义配置模型、默认值、支持模型列表、业务校验和 `load_config(path)`
- `src/main.rs`：`run()` 负责 `Cli::parse()`、`match Commands`、调用对应 `execute()`，并通过 `?` 转发错误；`main()` 负责调用 `run()`、统一打印错误、打印错误链和设置失败退出码
- `read-config` 当前用于配置读取和解析练习，负责调用 `config::load_config(path)` 并输出配置字段
- `Config.model` 当前通过 `#[serde(default = "default_model")]` 缺失时默认使用 `gpt-4.1`
- `Config.temperature` 当前通过 `#[serde(default = "default_temperature")]` 缺失时默认使用 `0.7`
- `Config::validate()` 当前校验模型支持列表和 `temperature` 范围
- `SUPPORTED_MODELS` 当前定义了允许的模型列表：`gpt-4.1`、`gpt-4.1-mini`
- `Config` 和需要跨模块读取的字段当前使用 `pub` 暴露给命令层
- `src/config.rs` 当前包含单元测试模块，测试配置默认值、业务校验失败和 TOML 字段类型错误

新增命令时应遵循：

1. 在 `src/cli.rs` 中定义命令参数
2. 在 `src/commands/` 下新增命令模块
3. 在 `src/commands/mod.rs` 中导出模块
4. 在 `src/main.rs` 中添加命令分发
5. 在命令模块中提供返回 `anyhow::Result<()>` 的 `execute()` 函数

## Open Questions

- TOML 解析失败时，错误信息应该暴露多少底层细节？
- `read-config` 是调试命令，还是未来真实 Agent 配置加载流程的一部分？
- 支持模型列表后续应该硬编码、放入配置，还是由 API 能力发现？
- 后续命令越来越多时，是否需要改进 `main.rs` 中的分发方式？
- 后续是否需要为命令执行结果和错误输出增加自动化测试？
- `Config` 字段长期保持 `pub`，还是后续改为访问器方法？
- `load_config()` 测试是否使用临时文件，还是先用简单测试辅助函数？

## Technical Debt

- 当前命令没有测试
- `config::load_config()` 当前还没有直接测试
- `rejects_unsupported_model` 测试数据中存在 `unkown-model` 拼写问题，后续应修正为 `unknown-model`
- 项目根目录存在 `config.toml`，需要决定它是示例配置、测试配置，还是临时文件
- 当前项目根目录没有 `PROJECT_STATUS.md`，实际状态文件位于 `journal/PROJECT_STATUS.md`
- 当前日志文件命名存在 `2026-7-13.md`、`2026-7-14.md`，后续建议统一为 `YYYY-MM-DD.md`
- 旧日志文件 `2026-7-14.md` 与标准命名 `2026-07-14.md` 同时存在，后续需要决定是否迁移或保留

## Next TODO

- [ ] 修正测试数据中的 `unknown-model` 拼写
- [ ] 为 `config::load_config()` 编写基础测试
- [ ] 测试 `load_config()` 的默认值、非法模型、非法 temperature 和坏 TOML
- [ ] 在配置测试稳定后，准备进入 `reqwest` 和 async Rust
