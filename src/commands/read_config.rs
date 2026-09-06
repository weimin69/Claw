use anyhow::Result;

pub fn execute(path: &str) -> Result<()> {
    let config = crate::config::load_config(path)?;

    println!("llm base url: {}", config.llm.base_url);
    println!("model: {}", config.llm.model);
    println!("temperature: {}", config.llm.temperature);

    if config.llm.api_key.trim().is_empty() {
        println!("llm api key: empty");
    } else {
        println!("llm api key: set");
    }

    Ok(())
}
