use anyhow::Result;

pub fn execute(path: &str) -> Result<()> {
    let config = crate::config::load_config(path)?;

    println!("model: {}", config.model);
    println!("temperature: {}", config.temperature);

    match &config.openai {
        Some(openai) if !openai.api_key.is_empty() => {
            println!("openai api key: set");
        }
        Some(_) => {
            println!("openai api key: empty");
        }
        None => {
            println!("openai api key: not set");
        }
    }
    Ok(())
}
