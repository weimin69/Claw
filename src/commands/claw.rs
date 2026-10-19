use crate::agent::{AgentRunResult, AgentSession, run_agent};
use crate::openai_compatible::OpenAiCompatibleAgentModel;
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
        answer = run_agent(&mut model, prompt, max_iterations, max_tool_calls, &mut tool_executor) => answer?,
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
) -> Result<()> {
    eprintln!("交互模式：每行一条消息，/exit 退出；会话仅保存在当前进程。");
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
    let mut session = AgentSession::default();
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
            result = session.run_turn(model, input, max_iterations, max_tool_calls, tools) => result?,
        };
        print_result(&result);
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
