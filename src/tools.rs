use crate::agent::{ToolExecutor, ToolOutput};
use crate::workspace::Workspace;
use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io::Read, path::Path};

mod command;
mod create;
mod edit;
mod search;
pub(crate) struct BuiltinToolExecutor {
    workspace: Workspace,
    allow_write: bool,
    allow_exec: bool,
}

impl BuiltinToolExecutor {
    pub(crate) fn new(workspace: Workspace) -> Self {
        Self {
            workspace,
            allow_write: false,
            allow_exec: false,
        }
    }

    pub(crate) fn with_write_access(mut self, allow_write: bool) -> Self {
        self.allow_write = allow_write;
        self
    }

    pub(crate) fn with_exec_access(mut self, allow_exec: bool) -> Self {
        self.allow_exec = allow_exec;
        self
    }
}

impl ToolExecutor for BuiltinToolExecutor {
    async fn execute(&mut self, name: &str, input: &str) -> Result<ToolOutput> {
        let result = match name {
            "add" => execute_add(input),
            "run_command" => {
                if !self.allow_exec {
                    return Ok(ToolOutput::error(
                        "execution access denied; enable --allow-exec to use run_command"
                            .to_string(),
                    ));
                }
                return Ok(match command::execute(&self.workspace, input).await {
                    Ok(output) => output,
                    Err(error) => ToolOutput::error(format!("{error:#}")),
                });
            }
            "create_file" => {
                if !self.allow_write {
                    return Ok(ToolOutput::error(
                        "write access denied; enable --allow-write to use create_file".to_string(),
                    ));
                }
                let workspace = self.workspace.clone();
                let input = input.to_string();
                tokio::task::spawn_blocking(move || create::execute(&workspace, &input))
                    .await
                    .context("create_file worker failed")?
            }
            "replace_text" => {
                if !self.allow_write {
                    return Ok(ToolOutput::error(
                        "write access denied; enable --allow-write to use replace_text".to_string(),
                    ));
                }
                let workspace = self.workspace.clone();
                let input = input.to_string();
                tokio::task::spawn_blocking(move || edit::execute(&workspace, &input))
                    .await
                    .context("replace_text worker failed")?
            }
            "list_files" => {
                let workspace = self.workspace.clone();
                let input = input.to_string();
                tokio::task::spawn_blocking(move || execute_list_files(&workspace, &input))
                    .await
                    .context("list_files worker failed")?
            }
            "read_file" => {
                let workspace = self.workspace.clone();
                let input = input.to_string();
                tokio::task::spawn_blocking(move || execute_read_file(&workspace, &input))
                    .await
                    .context("read_file worker failed")?
            }
            "search_code" => {
                let workspace = self.workspace.clone();
                let input = input.to_string();
                tokio::task::spawn_blocking(move || search::execute(&workspace, &input))
                    .await
                    .context("search_code worker failed")?
            }
            _ => return Ok(ToolOutput::error(format!("unknown tool `{name}`"))),
        };

        Ok(match result {
            Ok(output) => ToolOutput::success(output),
            Err(error) => ToolOutput::error(error.to_string()),
        })
    }
}

const MAX_LIST_ENTRIES: usize = 200;

const MAX_READ_FILE_BYTES: usize = 1024 * 1024;
const MAX_READ_OUTPUT_BYTES: usize = 32 * 1024;
const MAX_READ_LINES: usize = 500;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadFileInput {
    path: String,
    start_line: Option<usize>,
    end_line: Option<usize>,
}

fn execute_read_file(workspace: &Workspace, input: &str) -> Result<String> {
    let input: ReadFileInput =
        serde_json::from_str(input).context("invalid read_file tool input")?;
    if input.path.trim().is_empty() {
        bail!("read_file path must not be empty");
    }
    let start = input.start_line.unwrap_or(1);
    let end = input.end_line.unwrap_or_else(|| start.saturating_add(199));
    if start == 0 || end < start {
        bail!("read_file requires 1 <= start_line <= end_line");
    }
    let path = workspace.resolve_existing(Path::new(&input.path))?;
    if !path.is_file() {
        bail!("read_file path must be a regular file");
    }
    let file = std::fs::File::open(&path).context("failed to open read_file path")?;
    if !file
        .metadata()
        .context("failed to inspect read_file path")?
        .is_file()
    {
        bail!("read_file path must be a regular file");
    }
    // Enforce the bound on actual bytes read, including files that grow after opening.
    let mut bytes = Vec::new();
    file.take((MAX_READ_FILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .context("failed to read file")?;
    if bytes.len() > MAX_READ_FILE_BYTES {
        bail!("read_file file exceeds 1 MiB limit");
    }
    if bytes.contains(&0) {
        bail!("read_file does not support binary files (NUL byte found)");
    }
    let text = std::str::from_utf8(&bytes).context("read_file requires UTF-8 text")?;
    let lines: Vec<_> = text.split_inclusive('\n').collect();
    if start > lines.len() && !(lines.is_empty() && start == 1) {
        bail!("read_file start_line exceeds file length");
    }
    let mut content = String::new();
    let mut actual_end = start - 1;
    for (index, line) in lines
        .iter()
        .enumerate()
        .skip(start - 1)
        .take(MAX_READ_LINES)
    {
        if index >= end {
            break;
        }
        let available = MAX_READ_OUTPUT_BYTES - content.len();
        let mut length = line.len().min(available);
        while !line.is_char_boundary(length) {
            length -= 1;
        }
        if length == 0 {
            break;
        }
        content.push_str(&line[..length]);
        actual_end = index + 1;
        if length < line.len() {
            return read_file_result(workspace, &path, start, actual_end, content, true);
        }
    }
    let truncated = actual_end < lines.len();
    read_file_result(workspace, &path, start, actual_end, content, truncated)
}

fn read_file_result(
    workspace: &Workspace,
    path: &Path,
    start: usize,
    end: usize,
    content: String,
    truncated: bool,
) -> Result<String> {
    let path = path
        .strip_prefix(workspace.root())?
        .to_str()
        .context("read_file path is not UTF-8")?;
    Ok(serde_json::json!({
        "path": path, "start_line": start, "end_line": end,
        "content": content, "truncated": truncated
    })
    .to_string())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListFilesInput {
    path: String,
}

#[derive(Serialize)]
struct ListEntry {
    path: String,
    #[serde(rename = "type")]
    kind: &'static str,
}

fn execute_list_files(workspace: &Workspace, input: &str) -> Result<String> {
    let input: ListFilesInput =
        serde_json::from_str(input).context("invalid list_files tool input")?;
    if input.path.trim().is_empty() {
        bail!("list_files path must not be empty");
    }
    let directory = workspace.resolve_existing(Path::new(&input.path))?;
    if !directory.is_dir() {
        bail!("list_files path must be a directory");
    }
    let entries = std::fs::read_dir(&directory).context("failed to read directory")?;
    // Keep only the first 200 names in sorted order, without retaining the
    // whole directory in memory. Read every entry to make ordering stable.
    let mut selected = BTreeMap::new();
    let mut truncated = false;
    for entry in entries {
        let entry = entry.context("failed to read directory entry")?;
        let entry_path = entry.path();
        let relative = entry_path.strip_prefix(workspace.root())?;
        let path = relative
            .to_str()
            .context("directory entry path is not UTF-8")?
            .to_string();
        let file_type = entry
            .file_type()
            .context("failed to read directory entry type")?;
        let kind = if file_type.is_symlink() {
            "symlink"
        } else if file_type.is_dir() {
            "directory"
        } else if file_type.is_file() {
            "file"
        } else {
            "other"
        };
        selected.insert(path.clone(), ListEntry { path, kind });
        if selected.len() > MAX_LIST_ENTRIES {
            selected.pop_last();
            truncated = true;
        }
    }
    let entries: Vec<_> = selected.into_values().collect();
    Ok(serde_json::json!({"entries": entries, "truncated": truncated}).to_string())
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

    #[tokio::test]
    async fn replace_text_checks_permissions_before_parsing_or_writing() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("file");
        std::fs::write(&path, "old").unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();
        let mut executor = BuiltinToolExecutor::new(workspace);
        let denied = executor.execute("replace_text", "not json").await.unwrap();
        assert!(denied.is_error);
        assert!(denied.content.contains("write access denied"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old");
        let mut executor = executor.with_write_access(true);
        let output = executor
            .execute(
                "replace_text",
                r#"{"path":"file","old_text":"old","new_text":"new"}"#,
            )
            .await
            .unwrap();
        assert!(!output.is_error);
        let value: serde_json::Value = serde_json::from_str(&output.content).unwrap();
        assert_eq!(value["applied"], true);
        assert!(value["diff"].as_str().unwrap().contains("+new"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
        let output = executor
            .execute(
                "replace_text",
                r#"{"path":"file","old_text":"absent","new_text":"x"}"#,
            )
            .await
            .unwrap();
        assert!(output.is_error);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
    }

    #[test]
    fn read_file_preserves_text_and_selects_inclusive_lines() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("file"), "one\r\n你好\nlast").unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();
        let output =
            execute_read_file(&workspace, r#"{"path":"file","start_line":2,"end_line":3}"#)
                .unwrap();
        let value: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(
            value,
            serde_json::json!({"path":"file","start_line":2,"end_line":3,"content":"你好\nlast","truncated":false})
        );
        std::fs::write(directory.path().join("file"), "").unwrap();
        let value: serde_json::Value =
            serde_json::from_str(&execute_read_file(&workspace, r#"{"path":"file"}"#).unwrap())
                .unwrap();
        assert_eq!(value["content"], "");
        assert_eq!(value["end_line"], 0);
        assert_eq!(value["truncated"], false);
    }

    #[test]
    fn read_file_limits_lines_and_utf8_output_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();
        std::fs::write(directory.path().join("file"), "x\n".repeat(501)).unwrap();
        for (extra, expected) in [("", 200), (",\"end_line\":1000", 500)] {
            let input = format!("{{\"path\":\"file\"{extra}}}");
            let value: serde_json::Value =
                serde_json::from_str(&execute_read_file(&workspace, &input).unwrap()).unwrap();
            assert_eq!(value["end_line"], expected);
            assert_eq!(value["truncated"], true);
        }
        std::fs::write(directory.path().join("file"), "你".repeat(12000)).unwrap();
        let value: serde_json::Value =
            serde_json::from_str(&execute_read_file(&workspace, r#"{"path":"file"}"#).unwrap())
                .unwrap();
        let content = value["content"].as_str().unwrap();
        assert_eq!(content.len(), MAX_READ_OUTPUT_BYTES / 3 * 3);
        assert!(content.chars().all(|character| character == '你'));
        assert_eq!(value["truncated"], true);
    }

    #[tokio::test]
    async fn read_file_reports_invalid_inputs_and_files_as_recoverable_errors() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("file"), "text").unwrap();
        std::fs::write(directory.path().join("binary"), [0, 1]).unwrap();
        std::fs::write(directory.path().join("invalid-utf8"), [255]).unwrap();
        let large = std::fs::File::create(directory.path().join("large")).unwrap();
        large.set_len((MAX_READ_FILE_BYTES + 1) as u64).unwrap();
        let mut executor = BuiltinToolExecutor::new(Workspace::new(directory.path()).unwrap());
        for (input, expected) in [
            (r#"{}"#, "invalid read_file tool input"),
            (r#"{"path":""}"#, "must not be empty"),
            (r#"{"path":"file","start_line":0}"#, "1 <= start_line"),
            (
                r#"{"path":"file","start_line":2,"end_line":1}"#,
                "1 <= start_line",
            ),
            (r#"{"path":"file","start_line":2}"#, "exceeds file length"),
            (r#"{"path":"missing"}"#, "failed to resolve workspace path"),
            (r#"{"path":"."}"#, "regular file"),
            (r#"{"path":".."}"#, "outside workspace"),
            (r#"{"path":"binary"}"#, "binary files"),
            (r#"{"path":"invalid-utf8"}"#, "UTF-8"),
            (r#"{"path":"large"}"#, "1 MiB"),
        ] {
            let output = executor.execute("read_file", input).await.unwrap();
            assert!(output.is_error);
            assert!(output.content.contains(expected), "{}", output.content);
        }
    }

    #[tokio::test]
    async fn lists_sorted_workspace_relative_entries_without_recursion() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join("src")).unwrap();
        std::fs::write(directory.path().join("src/z.rs"), "").unwrap();
        std::fs::write(directory.path().join("src/a.rs"), "").unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();
        let mut executor = BuiltinToolExecutor::new(workspace);
        let output = executor
            .execute("list_files", r#"{"path":"src"}"#)
            .await
            .unwrap();
        assert!(!output.is_error);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&output.content).unwrap(),
            serde_json::json!({
                "entries": [{"path":"src/a.rs","type":"file"},{"path":"src/z.rs","type":"file"}],
                "truncated": false
            })
        );
    }

    #[test]
    fn lists_empty_directory_and_marks_truncation_at_boundary() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();
        let read = || {
            serde_json::from_str::<serde_json::Value>(
                &execute_list_files(&workspace, r#"{"path":"."}"#).unwrap(),
            )
            .unwrap()
        };
        assert_eq!(read(), serde_json::json!({"entries":[],"truncated":false}));
        for index in 0..200 {
            std::fs::write(directory.path().join(format!("{index:03}")), "").unwrap();
        }
        assert_eq!(read()["truncated"], false);
        std::fs::create_dir(directory.path().join("zzz")).unwrap();
        let value = read();
        assert_eq!(value["entries"].as_array().unwrap().len(), 200);
        assert_eq!(value["entries"][0]["path"], "000");
        assert_eq!(value["entries"][199]["path"], "199");
        assert_eq!(value["truncated"], true);
    }

    #[tokio::test]
    async fn list_files_returns_recoverable_errors() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("file"), "").unwrap();
        let mut executor = BuiltinToolExecutor::new(Workspace::new(directory.path()).unwrap());
        for (input, message) in [
            (r#"{}"#, "invalid list_files tool input"),
            (r#"{"path":""}"#, "must not be empty"),
            (r#"{"path":"missing"}"#, "failed to resolve workspace path"),
            (r#"{"path":"file"}"#, "must be a directory"),
            (r#"{"path":".."}"#, "outside workspace"),
        ] {
            let output = executor.execute("list_files", input).await.unwrap();
            assert!(output.is_error);
            assert!(output.content.contains(message), "{}", output.content);
        }
    }

    #[cfg(unix)]
    #[test]
    fn lists_symlinks_without_following_external_targets() {
        let directory = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), directory.path().join("link")).unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();
        let output = execute_list_files(&workspace, r#"{"path":"."}"#).unwrap();
        let value: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(
            value["entries"][0],
            serde_json::json!({"path":"link","type":"symlink"})
        );
        assert!(
            execute_list_files(&workspace, r#"{"path":"link"}"#)
                .unwrap_err()
                .to_string()
                .contains("outside workspace")
        );
    }

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
        let directory = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();
        let mut executor = BuiltinToolExecutor::new(workspace);

        let output = executor.execute("add", r#"{"a":2,"b":3}"#).await.unwrap();

        assert_eq!(output, ToolOutput::success("5".to_string()));
    }

    #[tokio::test]
    async fn builtin_executor_returns_recoverable_error_for_unknown_tool() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();
        let mut executor = BuiltinToolExecutor::new(workspace);

        let output = executor.execute("weather", "{}").await.unwrap();

        assert_eq!(
            output,
            ToolOutput::error("unknown tool `weather`".to_string())
        );
    }

    #[tokio::test]
    async fn builtin_executor_returns_recoverable_error_for_invalid_input() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();
        let mut executor = BuiltinToolExecutor::new(workspace);

        let output = executor.execute("add", r#"{"a":2}"#).await.unwrap();

        assert_eq!(
            output,
            ToolOutput::error("invalid add tool input".to_string())
        );
    }
}
