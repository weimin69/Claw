use super::MAX_READ_FILE_BYTES;
use crate::workspace::Workspace;
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::{io::Write, path::Path};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateFileInput {
    path: String,
    content: String,
}

pub(super) fn execute(workspace: &Workspace, input: &str) -> Result<String> {
    let input: CreateFileInput =
        serde_json::from_str(input).context("invalid create_file tool input")?;
    if input.path.trim().is_empty() {
        bail!("create_file path must not be empty");
    }
    if input.content.len() > MAX_READ_FILE_BYTES {
        bail!("create_file content exceeds 1 MiB limit");
    }
    if input.content.contains('\0') {
        bail!("create_file does not support NUL bytes");
    }
    let target = workspace.resolve_new_file(Path::new(&input.path))?;
    let relative = target
        .strip_prefix(workspace.root())?
        .to_str()
        .context("create_file path is not UTF-8")?;
    let result = serde_json::json!({
        "path":relative,"created":true,"bytes_written":input.content.len()
    })
    .to_string();
    let parent = target.parent().context("create_file path has no parent")?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).context("failed to create temporary new file")?;
    temporary
        .write_all(input.content.as_bytes())
        .context("failed to write new file content")?;
    temporary
        .as_file()
        .sync_all()
        .context("failed to sync new file")?;
    // Catch changed parent links before publishing. This application check is
    // not isolation against a concurrent directory replacement after the check.
    if workspace.resolve_new_file(Path::new(&input.path))? != target {
        bail!("create_file target path changed; retry the operation");
    }
    temporary
        .persist_noclobber(&target)
        .map_err(|error| error.error)
        .context("failed to create file without overwriting an existing target")?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{agent::ToolExecutor, tools::BuiltinToolExecutor};
    use std::fs;

    fn request(path: &str, content: &str) -> String {
        serde_json::json!({"path":path,"content":content}).to_string()
    }

    #[test]
    fn creates_utf8_file_in_existing_subdirectory_and_empty_file() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir(directory.path().join("src")).unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();
        let content = "你好\r\n";
        let result = execute(&workspace, &request("src/new.rs", content)).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&result).unwrap(),
            serde_json::json!({
                "path":"src/new.rs","created":true,"bytes_written":content.len()
            })
        );
        assert_eq!(
            fs::read(directory.path().join("src/new.rs")).unwrap(),
            content.as_bytes()
        );
        execute(&workspace, &request("empty", "")).unwrap();
        assert_eq!(
            fs::metadata(directory.path().join("empty")).unwrap().len(),
            0
        );
        assert_eq!(
            fs::read_dir(directory.path().join("src")).unwrap().count(),
            1
        );
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 2);
    }

    #[test]
    fn never_overwrites_existing_files_or_directories() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();
        fs::write(directory.path().join("file"), "user content").unwrap();
        fs::create_dir(directory.path().join("folder")).unwrap();
        for path in ["file", "folder"] {
            assert!(
                execute(&workspace, &request(path, "new"))
                    .unwrap_err()
                    .to_string()
                    .contains("already exists")
            );
        }
        assert_eq!(
            fs::read_to_string(directory.path().join("file")).unwrap(),
            "user content"
        );
        assert!(directory.path().join("folder").is_dir());
    }

    #[test]
    fn rejects_invalid_paths_content_and_input_without_creating_files() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();
        for path in [
            "",
            ".",
            "..",
            "x/..",
            "folder/.",
            "folder/",
            "missing/file",
            "../escape",
        ] {
            assert!(
                execute(&workspace, &request(path, "text")).is_err(),
                "{path}"
            );
        }
        assert!(execute(&workspace, "{}").is_err());
        assert!(execute(&workspace, r#"{"path":"file","content":"x","extra":true}"#).is_err());
        assert!(execute(&workspace, &request("file", "\0")).is_err());
        assert!(
            execute(
                &workspace,
                &request("file", &"x".repeat(MAX_READ_FILE_BYTES + 1))
            )
            .is_err()
        );
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_existing_symlinks_and_parent_escape() {
        use std::os::unix::fs::symlink;
        let directory = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();
        symlink(
            outside.path().join("missing"),
            directory.path().join("dangling"),
        )
        .unwrap();
        symlink(outside.path(), directory.path().join("external")).unwrap();
        for path in ["dangling", "external/file"] {
            assert!(execute(&workspace, &request(path, "text")).is_err());
        }
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
        assert!(
            fs::symlink_metadata(directory.path().join("dangling"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[tokio::test]
    async fn executor_enforces_authorization_and_returns_recoverable_errors() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();
        let mut executor = BuiltinToolExecutor::new(workspace);
        let output = executor.execute("create_file", "not JSON").await.unwrap();
        assert!(output.is_error);
        assert!(output.content.contains("write access denied"));
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
        let mut executor = executor.with_write_access(true);
        let input = request("file", "created");
        assert!(
            !executor
                .execute("create_file", &input)
                .await
                .unwrap()
                .is_error
        );
        assert!(
            executor
                .execute("create_file", &input)
                .await
                .unwrap()
                .is_error
        );
        assert_eq!(
            fs::read_to_string(directory.path().join("file")).unwrap(),
            "created"
        );
    }
}
