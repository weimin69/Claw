use anyhow::Result;

pub fn execute(path: &str) -> Result<()> {
    let config = crate::config::load_config(path)?;

    println!("model: {}", config.model);
    println!("temperature: {}", config.temperature);

    Ok(())
}
