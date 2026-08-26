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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_default_values_when_fields_are_missing() {
        let config: Config = toml::from_str("").unwrap();

        assert_eq!(config.model, "gpt-4.1");
        assert_eq!(config.temperature, 0.7);
    }

    #[test]
    fn rejects_unsupported_model() {
        let config = Config {
            model: "unknown-model".to_string(),
            temperature: 0.7,
        };

        let result = config.validate();
        assert!(result.is_err());
    }

    #[test]
    fn rejects_temperature_out_of_range() {
        let config = Config {
            model: "gpt-4.1".to_string(),
            temperature: 3.0,
        };

        let result = config.validate();

        assert!(result.is_err());
    }

    #[test]
    fn rejects_invalid_temperature_type() {
        let result = toml::from_str::<Config>(r#"temperature ="hot""#);

        assert!(result.is_err());
    }

    #[test]
    fn loads_config_from_file() {
        let file = write_temp_config(
            r#"
      model = "gpt-4.1-mini"
      temperature = 1.0
      "#,
        );

        let config = load_config(file.path().to_str().unwrap()).unwrap();

        assert_eq!(config.model, "gpt-4.1-mini");
        assert_eq!(config.temperature, 1.0);
    }
    #[test]
    fn load_config_applies_default_values_when_fields_are_missing() {
        let file = write_temp_config("");

        let config = load_config(file.path().to_str().unwrap()).unwrap();

        assert_eq!(config.model, "gpt-4.1");
        assert_eq!(config.temperature, 0.7);
    }

    #[test]
    fn load_config_rejects_unsupported_model() {
        let file = write_temp_config(
            r#"
      model = "unknown-model"
      temperature = 0.7
      "#,
        );

        let result = load_config(file.path().to_str().unwrap());

        assert!(result.is_err());
    }

    #[test]
    fn load_config_rejects_temperature_out_of_range() {
        let file = write_temp_config(
            r#"
      model = "gpt-4.1"
      temperature = 3.0
      "#,
        );

        let result = load_config(file.path().to_str().unwrap());

        assert!(result.is_err());
    }

    #[test]
    fn load_config_adds_context_when_toml_parse_fails() {
        let file = write_temp_config(
            r#"
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
