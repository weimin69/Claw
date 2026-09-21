use anyhow::{Context, Result, bail};
use std::io::{Read, stdin};

pub async fn execute(config_path: &str, prompt: Option<String>) -> Result<()> {
    fn read_prompt_from_stdin() -> Result<String> {
        let mut content = String::new();
        let mut stdin = stdin();
        stdin
            .read_to_string(&mut content)
            .context("failed to read prompt from stdin")?;

        Ok(content)
    }

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

    let text = crate::openai_compatible::send_chat_request(
        &llm.base_url,
        &llm.api_key,
        llm.model,
        llm.temperature,
        prompt,
    )
    .await?;

    println!("{}", text);
    Ok(())
}
