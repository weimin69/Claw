use crate::agent::run_agent;
use crate::openai_compatible::OpenAiCompatibleAgentModel;
use crate::tools::BuiltinToolExecutor;
use anyhow::{Context, Result, bail};
use std::io::{Read, stdin};

#[derive(Debug)]
pub(crate) struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("claw task cancelled")
    }
}

impl std::error::Error for Cancelled {}

pub async fn execute(
    config_path: &str,
    prompt: Option<String>,
    workspace_path: &std::path::Path,
    allow_write: bool,
    allow_exec: bool,
    max_iterations: Option<usize>,
    max_tool_calls: Option<usize>,
) -> Result<()> {
    let prompt = match prompt {
        Some(value) => value,
        None => read_prompt_from_stdin()?,
    };

    if prompt.trim().is_empty() {
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

    let answer = tokio::select! {
        biased;
        signal = tokio::signal::ctrl_c() => {
            signal.context("failed to listen for Ctrl+C")?;
            return Err(Cancelled.into());
        }
        answer = run_agent(&mut model, prompt, max_iterations, max_tool_calls, &mut tool_executor) => answer?,
    };

    println!("{answer}");
    Ok(())
}

fn read_prompt_from_stdin() -> Result<String> {
    let mut content = String::new();
    stdin()
        .read_to_string(&mut content)
        .context("failed to read prompt from stdin")?;

    Ok(content)
}
