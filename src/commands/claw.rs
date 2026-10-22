use crate::agent::{AgentEvent, AgentRunResult, AgentSession, run_agent};
use crate::openai_compatible::OpenAiCompatibleAgentModel;
use crate::session::SessionStore;
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
    let (store, session) = if let Some(path) = resume {
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
            session_file
                .map(|path| SessionStore::new(path, workspace.root()))
                .transpose()?,
            AgentSession::default(),
        )
    };
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
        biased;
        signal = tokio::signal::ctrl_c() => {
            signal.context("failed to listen for Ctrl+C")?;
            return Err(Cancelled.into());
        }
        answer = run_agent(&mut model, prompt, max_iterations, max_tool_calls, &mut tool_executor, print_event) => answer?,
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
    eprintln!("交互模式：每行一条消息，/exit 退出；只有指定存档参数才保存到磁盘。");
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
    loop {
        let input = match prompt.take() {
            Some(input) => input,
            None => {
                eprint!("你> ");
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
        if input.trim().is_empty() {
            continue;
        }
        let result = tokio::select! {
            biased;
            signal = tokio::signal::ctrl_c() => {
                signal.context("failed to listen for Ctrl+C")?;
                return Err(Cancelled.into());
            }
            result = session.run_turn(model, input, max_iterations, max_tool_calls, tools, print_event) => result?,
        };
        print_result(&result);
        if let Some(mut writer) = store.take() {
            let messages = session.completed_messages()?.to_vec();
            writer = tokio::task::spawn_blocking(move || {
                writer.save(messages)?;
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
