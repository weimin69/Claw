use anyhow::{Result, bail};

pub async fn execute(config_path: &str, prompt: &str) -> Result<()> {
    let config = crate::config::load_config(config_path)?;
    let llm = config.llm;

    if llm.api_key.trim().is_empty() {
        bail!("missing llm api key");
    }

    let text = crate::openai::send_chat_request(
        &llm.base_url,
        &llm.api_key,
        llm.model,
        llm.temperature,
        prompt.to_string(),
    )
    .await?;

    println!("{}", text);
    Ok(())
}
