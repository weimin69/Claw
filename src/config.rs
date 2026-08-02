use anyhow::{Context, Result, bail};
use serde::Deserialize;

const SUPPORTED_MODELS: &[&str] = &["gpt-4.1", "gpt-4.1-mini"];

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default = "default_model")]
    pub model: String,

    #[serde(default = "default_temperature")]
    pub temperature: f64,
}

pub fn load_config(path: &str) -> Result<Config> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read config file: {}", path))?;

    let config: Config = toml::from_str(&content).context("failed to parse config file")?;

    config.validate()?;

    Ok(config)
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
