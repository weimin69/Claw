use crate::agent::{ToolExecutor, ToolOutput};
use anyhow::{Context, Result, anyhow};
use serde::Deserialize;

pub(crate) struct BuiltinToolExecutor;

impl ToolExecutor for BuiltinToolExecutor {
    async fn execute(&mut self, name: &str, input: &str) -> Result<ToolOutput> {
        let result = match name {
            "add" => execute_add(input),
            _ => return Ok(ToolOutput::error(format!("unknown tool `{name}`"))),
        };

        Ok(match result {
            Ok(output) => ToolOutput::success(output),
            Err(error) => ToolOutput::error(error.to_string()),
        })
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

    #[tokio::test]
    async fn builtin_executor_runs_add_tool() {
        let mut executor = BuiltinToolExecutor;

        let output = executor.execute("add", r#"{"a":2,"b":3}"#).await.unwrap();

        assert_eq!(output, ToolOutput::success("5".to_string()));
    }

    #[tokio::test]
    async fn builtin_executor_returns_recoverable_error_for_unknown_tool() {
        let mut executor = BuiltinToolExecutor;

        let output = executor.execute("weather", "{}").await.unwrap();

        assert_eq!(
            output,
            ToolOutput::error("unknown tool `weather`".to_string())
        );
    }

    #[tokio::test]
    async fn builtin_executor_returns_recoverable_error_for_invalid_input() {
        let mut executor = BuiltinToolExecutor;

        let output = executor.execute("add", r#"{"a":2}"#).await.unwrap();

        assert_eq!(
            output,
            ToolOutput::error("invalid add tool input".to_string())
        );
    }
}
