use anyhow::{Context, Result, bail};
use serde::Deserialize;
pub fn execute(path: &str) -> Result<()> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read config file: {}", path))?;

    let config: Config = toml::from_str(&content).context("failed to parse config file")?;

    config.validate()?;
    println!("model: {}", config.model);

    println!("temperature: {}", config.temperature);

    Ok(())
}

#[derive(Debug, Deserialize)]
struct Config {
    #[serde(default = "default_model")]
    model: String,
    #[serde(default = "default_temperature")]
    temperature: f64,
}

fn default_model() -> String {
    "gpt-4.1".to_string()
}

fn default_temperature() -> f64 {
    0.7
}

impl Config {
    fn validate(&self) -> Result<()> {
        if !SUPPORTED_MODELS.contains(&self.model.as_str()) {
            bail!(
                "unsupported model: {}. supported models: {}",
                self.model,
                SUPPORTED_MODELS.join(", ")
            );
        }

        if self.temperature < 0.0 || self.temperature > 2.0 {
            bail!("temperature must be between 0.0 and 2.0");
        }

        Ok(())
    }
}
const SUPPORTED_MODELS: &[&str] = &["gpt-4.1", "gpt-4.1-mini"];
