use anyhow::{Context, Result, bail};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct LlmConfig {
    #[serde(default = "default_base_url")]
    pub base_url: String,

    #[serde(default)]
    pub api_key: String,

    #[serde(default = "default_model")]
    pub model: String,

    #[serde(default = "default_temperature")]
    pub temperature: f64,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            base_url: default_base_url(),
            api_key: String::new(),
            model: default_model(),
            temperature: default_temperature(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub llm: LlmConfig,
}

pub fn load_config(path: &str) -> Result<Config> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read config file: {}", path))?;

    let config: Config = toml::from_str(&content).context("failed to parse config file")?;

    config.validate()?;

    Ok(config)
}

fn default_base_url() -> String {
    "https://api.openai.com/v1".to_string()
}

fn default_model() -> String {
    "gpt-4.1".to_string()
}

fn default_temperature() -> f64 {
    0.7
}

impl Config {
    fn validate(&self) -> Result<()> {
        if self.llm.model.trim().is_empty() {
            bail!("model must not be empty");
        }

        if self.llm.temperature < 0.0 || self.llm.temperature > 2.0 {
            bail!("temperature must be between 0.0 and 2.0");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_default_values_when_fields_are_missing() {
        let config: Config = toml::from_str("").unwrap();

        assert_eq!(config.llm.base_url, "https://api.openai.com/v1");
        assert_eq!(config.llm.api_key, "");
        assert_eq!(config.llm.model, "gpt-4.1");
        assert_eq!(config.llm.temperature, 0.7);
    }

    #[test]
    fn rejects_empty_model() {
        let config = Config {
            llm: LlmConfig {
                base_url: "https://api.deepseek.com".to_string(),
                api_key: String::new(),
                model: "   ".to_string(),
                temperature: 0.7,
            },
        };

        let result = config.validate();
        assert!(result.is_err());
    }

    #[test]
    fn rejects_temperature_out_of_range() {
        let config = Config {
            llm: LlmConfig {
                base_url: "https://api.deepseek.com".to_string(),
                api_key: String::new(),
                model: "deepseek-v4-pro".to_string(),
                temperature: 3.0,
            },
        };

        let result = config.validate();

        assert!(result.is_err());
    }

    #[test]
    fn rejects_invalid_temperature_type() {
        let result = toml::from_str::<Config>(
            r#"[llm]
temperature = "hot"
"#,
        );

        assert!(result.is_err());
    }

    #[test]
    fn loads_config_from_file() {
        let file = write_temp_config(
            r#"[llm]
base_url = "https://api.deepseek.com"
api_key = "test-key"
model = "deepseek-v4-pro"
temperature = 1.0
"#,
        );

        let config = load_config(file.path().to_str().unwrap()).unwrap();

        assert_eq!(config.llm.base_url, "https://api.deepseek.com");
        assert_eq!(config.llm.api_key, "test-key");
        assert_eq!(config.llm.model, "deepseek-v4-pro");
        assert_eq!(config.llm.temperature, 1.0);
    }

    #[test]
    fn load_config_applies_default_values_when_fields_are_missing() {
        let file = write_temp_config("");

        let config = load_config(file.path().to_str().unwrap()).unwrap();

        assert_eq!(config.llm.base_url, "https://api.openai.com/v1");
        assert_eq!(config.llm.api_key, "");
        assert_eq!(config.llm.model, "gpt-4.1");
        assert_eq!(config.llm.temperature, 0.7);
    }

    #[test]
    fn load_config_rejects_empty_model() {
        let file = write_temp_config(
            r#"[llm]
model = ""
temperature = 0.7
"#,
        );

        let result = load_config(file.path().to_str().unwrap());

        assert!(result.is_err());
    }

    #[test]
    fn load_config_rejects_temperature_out_of_range() {
        let file = write_temp_config(
            r#"[llm]
model = "deepseek-v4-pro"
temperature = 3.0
"#,
        );

        let result = load_config(file.path().to_str().unwrap());

        assert!(result.is_err());
    }

    #[test]
    fn load_config_adds_context_when_toml_parse_fails() {
        let file = write_temp_config(
            r#"[llm]
temperature = "hot"
"#,
        );

        let error = load_config(file.path().to_str().unwrap()).unwrap_err();

        assert!(error.to_string().contains("failed to parse config file"));
    }

    #[test]
    fn load_config_adds_context_when_file_read_fails() {
        let error = load_config("missing-config-file.toml").unwrap_err();

        assert!(
            error
                .to_string()
                .contains("failed to read config file: missing-config-file.toml")
        );
    }

    fn write_temp_config(contents: &str) -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut file, contents.as_bytes()).unwrap();
        file
    }
}
