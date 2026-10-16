use crate::agent::run_agent;
use crate::openai_compatible::OpenAiCompatibleAgentModel;
use crate::tools::BuiltinToolExecutor;
use anyhow::{Context, Result, bail};
use std::io::{Read, stdin};

pub async fn execute(
    config_path: &str,
    prompt: Option<String>,
    max_iterations: usize,
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

    let mut model =
        OpenAiCompatibleAgentModel::new(llm.base_url, llm.api_key, llm.model, llm.temperature)?;
    let mut tool_executor = BuiltinToolExecutor;

    let answer = run_agent(&mut model, prompt, max_iterations, &mut tool_executor).await?;

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
