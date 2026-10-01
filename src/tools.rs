use crate::agent::ToolExecutor;
use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;

pub(crate) struct BuiltinToolExecutor;

impl ToolExecutor for BuiltinToolExecutor {
    async fn execute(&mut self, name: &str, input: &str) -> Result<String> {
        match name {
            "add" => execute_add(input),
            _ => bail!("unknown tool `{name}`"),
        }
    }
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct AddInput {
    a: i64,
    b: i64,
}

fn execute_add(input: &str) -> Result<String> {
    let parsed: AddInput = serde_json::from_str(input).context("invalid add tool input")?;

    let sum = parsed
        .a
        .checked_add(parsed.b)
        .ok_or_else(|| anyhow!("add result overflow"))?;

    Ok(sum.to_string())
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn parses_add_input() {
        let input: AddInput = serde_json::from_str(
            r#"{
            "a":2, "b":3
            }"#,
        )
        .unwrap();

        assert_eq!(input, AddInput { a: 2, b: 3 });
    }

    #[test]
    fn executes_add_tool() {
        let result = execute_add(r#"{"a":2,"b":3}"#).unwrap();
        assert_eq!(result, "5");
    }

    #[test]
    fn reject_invalid_add_input() {
        let error = execute_add(r#"{"a":2}"#).unwrap_err();
        assert_eq!(error.to_string(), "invalid add tool input");
    }

    #[test]
    fn rejects_add_overflow() {
        let error = execute_add(r#"{"a":9223372036854775807,"b":1}"#).unwrap_err();

        assert_eq!(error.to_string(), "add result overflow");
    }
}
