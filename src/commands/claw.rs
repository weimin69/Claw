use crate::agent::{AgentEvent, AgentRunResult, AgentSession, ContextLimitExceeded, run_agent};
use crate::openai_compatible::OpenAiCompatibleAgentModel;
use crate::session::{SessionStore, list_sessions};
use crate::tools::BuiltinToolExecutor;
use anyhow::{Context, Result, bail};
use std::io::{Read, Write, stdin};

#[derive(Debug)]
pub(crate) struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("claw task cancelled")
    }
}

impl std::error::Error for Cancelled {}

pub(crate) struct ClawOptions<'a> {
    pub(crate) workspace_path: &'a std::path::Path,
    pub(crate) allow_write: bool,
    pub(crate) allow_exec: bool,
    pub(crate) max_iterations: Option<usize>,
    pub(crate) max_tool_calls: Option<usize>,
    pub(crate) max_context_bytes: usize,
    pub(crate) interactive: bool,
    pub(crate) session_file: Option<&'a std::path::Path>,
    pub(crate) resume: Option<&'a std::path::Path>,
}

pub async fn execute(
    config_path: &str,
    prompt: Option<String>,
    options: ClawOptions<'_>,
) -> Result<()> {
    let ClawOptions {
        workspace_path,
        allow_write,
        allow_exec,
        max_iterations,
        max_tool_calls,
        max_context_bytes,
        interactive,
        session_file,
        resume,
    } = options;

    let prompt = match (prompt, interactive) {
        (Some(value), _) => Some(value),
        (None, false) => Some(read_prompt_from_stdin()?),
        (None, true) => None,
    };

    if prompt.as_ref().is_some_and(|value| value.trim().is_empty()) {
        bail!("prompt cannot be empty");
    }

    let config = crate::config::load_config(config_path)?;
    let llm = config.llm;

    if llm.api_key.trim().is_empty() {
        bail!("missing llm api key");
    }

    let workspace = crate::workspace::Workspace::new(workspace_path)?;
    let (store, mut session) = if let Some(path) = resume {
        let path = path.to_path_buf();
        let root = workspace.root().to_path_buf();
        let (store, session) =
            tokio::task::spawn_blocking(move || SessionStore::resume(&path, &root))
                .await
                .context("session load worker failed")??;
        eprintln!("已恢复会话；写入和执行权限由本次启动参数决定。历史不代表当前磁盘状态。");
        (Some(store), session)
    } else {
        (
            if let Some(path) = session_file {
                Some(SessionStore::new(path, workspace.root())?)
            } else if interactive {
                let root = workspace.root().to_path_buf();
                Some(
                    tokio::task::spawn_blocking(move || SessionStore::automatic(&root))
                        .await
                        .context("session allocation worker failed")??,
                )
            } else {
                None
            },
            AgentSession::default(),
        )
    };
    session.set_context_limit(max_context_bytes)?;
    let mut model =
        OpenAiCompatibleAgentModel::new(llm.base_url, llm.api_key, llm.model, llm.temperature)?
            .with_write_access(allow_write)
            .with_exec_access(allow_exec);
    let mut tool_executor = BuiltinToolExecutor::new(workspace)
        .with_write_access(allow_write)
        .with_exec_access(allow_exec);

    if interactive {
        return run_interactive(
            &mut model,
            &mut tool_executor,
            prompt,
            max_iterations,
            max_tool_calls,
            store,
            session,
        )
        .await;
    }
    let prompt = prompt.context("missing prompt")?;

    let result = tokio::select! {
        biased;//？
        signal = tokio::signal::ctrl_c() => {
            signal.context("failed to listen for Ctrl+C")?;
            return Err(Cancelled.into());
        }
        answer = run_agent(&mut model, prompt, max_iterations, max_tool_calls, &mut tool_executor, print_event, max_context_bytes) => answer?,
    };

    print_result(&result);
    Ok(())
}

async fn run_interactive(
    model: &mut OpenAiCompatibleAgentModel,
    tools: &mut BuiltinToolExecutor,
    mut prompt: Option<String>,
    max_iterations: Option<usize>,
    max_tool_calls: Option<usize>,
    mut store: Option<SessionStore>,
    mut session: AgentSession,
) -> Result<()> {
    eprintln!(
        "交互模式：每行一条消息，/sessions 查看存档，/resume <编号> 恢复，/new 开启新对话，/exit 退出；完成的对话自动保存。"
    );
    if let Some(writer) = &store {
        eprintln!("会话存档：{}", writer.path().display());
    }
    // A dedicated reader avoids blocking the runtime and Tokio stdin's
    // non-cancellable read preventing shutdown when Ctrl+C is pressed.
    let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
    std::thread::spawn(move || {
        loop {
            let mut line = String::new();
            match stdin().read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    if sender.blocking_send(Ok(line)).is_err() {
                        break;
                    }
                }
                Err(error) => {
                    let _ = sender.blocking_send(Err(error));
                    break;
                }
            }
        }
    });
    let mut listed_paths: Vec<std::path::PathBuf> = Vec::new();
    loop {
        let input = match prompt.take() {
            Some(input) => input,
            None => {
                eprint!(">#> ");
                std::io::stderr().flush()?;
                let line = tokio::select! {
                    biased;
                    signal = tokio::signal::ctrl_c() => {
                        signal.context("failed to listen for Ctrl+C")?;
                        return Err(Cancelled.into());
                    }
                    line = receiver.recv() => line,
                };
                let Some(line) = line else {
                    return Ok(());
                };
                line.context("failed to read interactive input")?
            }
        };
        if input.trim() == "/exit" {
            return Ok(());
        }
        if input.trim() == "/sessions" {
            let root = store
                .as_ref()
                .context("interactive session has no store")?
                .workspace()
                .to_path_buf();
            let archive_directory = root.join(".agent-cli/sessions");
            listed_paths.clear();
            let listing = tokio::task::spawn_blocking(move || list_sessions(&root))
                .await
                .context("session listing worker failed")?;
            match listing {
                Ok(listing) => {
                    eprintln!("当前 workspace 的自动存档（创建时间为 Unix 毫秒）：");
                    if listing.entries.is_empty() {
                        eprintln!("暂无可显示的存档。");
                    }
                    for (index, entry) in listing.entries.iter().enumerate() {
                        listed_paths.push(archive_directory.join(&entry.filename));
                        eprintln!(
                            "{}. {} | {} | {}",
                            index + 1,
                            entry.created_ms,
                            entry.preview.escape_debug(),
                            entry.filename.escape_debug()
                        );
                    }
                    if listing.skipped > 0 {
                        eprintln!("已跳过 {} 个无效或不可读取的存档。", listing.skipped);
                    }
                    if listing.incomplete {
                        eprintln!(
                            "列表未完整显示：最多扫描 1000 项、读取 32 MiB、显示 20 个存档；按已扫描文件的创建时间倒序排列。"
                        );
                    }
                    eprintln!("输入 /resume <编号> 恢复此列表中的对话。");
                }
                Err(error) => eprintln!("无法列出会话，当前对话不变：{error:#}"),
            }
            continue;
        }
        let mut parts = input.split_whitespace();
        if parts.next() == Some("/resume") {
            let index = parts
                .next()
                .and_then(|value| value.parse::<usize>().ok())
                .and_then(|value| value.checked_sub(1));
            let selected = index.and_then(|index| listed_paths.get(index)).cloned();
            if parts.next().is_some() || selected.is_none() {
                eprintln!("用法：/resume <有效编号>；请先用 /sessions 查看列表。当前对话不变。");
                continue;
            }
            let path = selected.context("missing selected session")?;
            let root = store
                .as_ref()
                .context("interactive session has no store")?
                .workspace()
                .to_path_buf();
            let budget = session.context_limit();
            let loaded = tokio::task::spawn_blocking(move || {
                let (writer, mut restored) = SessionStore::resume(&path, &root)?;
                restored.set_context_limit(budget)?;
                Ok::<_, anyhow::Error>((writer, restored))
            })
            .await
            .context("session resume worker failed")
            .and_then(|result| result);
            match loaded {
                Ok((writer, restored)) => {
                    eprintln!(
                        "已恢复所选对话：{}；权限和预算保持本次设置。",
                        writer.path().display()
                    );
                    session = restored;
                    store = Some(writer);
                }
                Err(error) => eprintln!("无法恢复会话，当前对话已保留：{error:#}"),
            }
            continue;
        }
        if input.trim() == "/new" {
            let root = store
                .as_ref()
                .context("interactive session has no store")?
                .workspace()
                .to_path_buf();
            // Every completed turn was saved before accepting the next input.
            // Allocate first: a failure must not discard the current conversation.
            let next = tokio::task::spawn_blocking(move || SessionStore::automatic(&root))
                .await
                .context("session allocation worker failed")?;
            match next {
                Ok(writer) => {
                    session = session.new_conversation();
                    eprintln!("已开启新对话。会话存档：{}", writer.path().display());
                    store = Some(writer);
                }
                Err(error) => eprintln!("无法开启新对话，当前会话已保留：{error:#}"),
            }
            continue;
        }
        if input.trim().is_empty() {
            continue;
        }
        let result = tokio::select! {
            biased;
            signal = tokio::signal::ctrl_c() => {
                signal.context("failed to listen for Ctrl+C")?;
                return Err(Cancelled.into());
            }
            result = session.run_turn(model, input, max_iterations, max_tool_calls, tools, print_event) => result,
        };
        let result = match result {
            Ok(result) => result,
            Err(error) if error.is::<ContextLimitExceeded>() && session.can_continue() => {
                eprintln!(
                    "{error}；本次输入未发送，原历史已保留。请缩短输入，或退出后开启新会话/调整预算。"
                );
                continue;
            }
            Err(error) => {
                if error.is::<ContextLimitExceeded>() {
                    eprintln!(
                        "工具执行后上下文超限，停止当前会话；保留上次成功存档，已执行的文件或命令操作不会回滚。"
                    );
                }
                return Err(error);
            }
        };
        print_result(&result);
        if let Some(mut writer) = store.take() {
            let messages = session.completed_messages()?.to_vec();
            let context = session.completed_context()?.to_vec();
            writer = tokio::task::spawn_blocking(move || {
                writer.save(messages, context)?;
                Ok::<_, anyhow::Error>(writer)
            })
            .await
            .context("answer completed, but session save worker failed")?
            .context("answer completed, but session save failed")?;
            store = Some(writer);
            eprintln!("会话已保存。存档可能包含源码与敏感工具输出，请勿提交到 Git。");
        }
    }
}

fn print_event(event: AgentEvent) {
    match event {
        AgentEvent::ContextTrimmed { turns } => {
            eprintln!("上下文已移除最旧的 {turns} 轮对话；模型不再接收这些内容，完整历史仍保留。")
        }
        AgentEvent::ModelStarted { iteration } => eprintln!("[第 {iteration} 轮] 等待模型响应……"),
        AgentEvent::ModelFinished {
            iteration,
            elapsed,
            is_error,
        } => {
            let status = if is_error { "失败" } else { "完成" };
            eprintln!(
                "[第 {iteration} 轮] 模型响应{status}，耗时 {:.3}s",
                elapsed.as_secs_f64()
            );
        }
        AgentEvent::ToolStarted { name } => eprintln!("[工具 {}] 开始", name.escape_debug()),
        AgentEvent::ToolFinished {
            name,
            elapsed,
            is_error,
        } => {
            let status = if is_error { "失败" } else { "完成" };
            eprintln!(
                "[工具 {}] {status}，耗时 {:.3}s",
                name.escape_debug(),
                elapsed.as_secs_f64()
            );
        }
    }
}

fn print_result(result: &AgentRunResult) {
    println!("{}", result.answer);
    println!("\n执行统计：");
    println!("工具调用：");
    if result.tool_calls.is_empty() {
        println!("  无");
    } else {
        for (name, count) in &result.tool_calls {
            println!("  {}: {count} 次", name.escape_debug());
        }
    }
    println!(
        "工具调用总数：{}",
        result.tool_calls.values().sum::<usize>()
    );
    println!("Loop 次数：{}", result.iterations);
}

fn read_prompt_from_stdin() -> Result<String> {
    let mut content = String::new();
    stdin()
        .read_to_string(&mut content)
        .context("failed to read prompt from stdin")?;

    Ok(content)
}
