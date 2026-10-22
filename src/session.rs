//! Local session file format, separate from provider-specific HTTP messages.
//! Histories may contain source code and sensitive tool output. This format
//! intentionally has no credentials or execution authorization fields.
use anyhow::{Context, Result, bail};
use std::{
    io::Write,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::agent::AgentMessage;

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SessionFile {
    pub(crate) version: u32,
    /// Canonical workspace path; the future loader must validate it on restore.
    pub(crate) workspace: PathBuf,
    pub(crate) messages: Vec<AgentMessage>,
}

/// A writer owns only the file it first created. Existing paths are never adopted.
/// Content checks detect ordinary concurrent changes, not hostile filesystem races.
pub(crate) struct SessionStore {
    path: PathBuf,
    workspace: PathBuf,
    last_saved: Option<Vec<u8>>,
}

impl SessionStore {
    pub(crate) fn new(path: &Path, workspace: &Path) -> Result<Self> {
        let name = path
            .file_name()
            .context("session file must have a file name")?;
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let parent = parent
            .canonicalize()
            .context("session file parent directory must exist")?;
        if !parent.is_dir() {
            bail!("session file parent must be a directory");
        }
        let path = parent.join(name);
        match std::fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("failed to inspect session file"),
            Ok(_) => bail!("session file already exists; choose a new path"),
        }
        Ok(Self {
            path,
            workspace: workspace.to_path_buf(),
            last_saved: None,
        })
    }

    pub(crate) fn save(&mut self, messages: Vec<AgentMessage>) -> Result<()> {
        let snapshot = SessionFile {
            version: 1,
            workspace: self.workspace.clone(),
            messages,
        };
        let bytes = serde_json::to_vec_pretty(&snapshot).context("failed to serialize session")?;
        let parent = self.path.parent().context("missing session directory")?;
        let mut temp = tempfile::NamedTempFile::new_in(parent)
            .context("failed to create session temporary file")?;
        // NamedTempFile is created with mode 0600 on Unix. Replacements retain
        // the temporary file's private permissions, not an older file's mode.
        temp.write_all(&bytes)
            .context("failed to write session temporary file")?;
        temp.as_file()
            .sync_all()
            .context("failed to sync session temporary file")?;
        if let Some(previous) = &self.last_saved {
            let metadata =
                std::fs::symlink_metadata(&self.path).context("saved session file disappeared")?;
            if !metadata.file_type().is_file()
                || metadata.len() != previous.len() as u64
                || metadata.permissions().readonly()
                || std::fs::read(&self.path).context("failed to check saved session")? != *previous
            {
                bail!("session file changed outside this session; refusing to overwrite");
            }
            temp.persist(&self.path)
                .map_err(|e| e.error)
                .context("failed to replace session file")?;
        } else {
            temp.persist_noclobber(&self.path)
                .map_err(|e| e.error)
                .context("failed to create session file without overwriting")?;
        }
        self.last_saved = Some(bytes);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::ToolCall;
    use serde_json::json;

    #[test]
    fn saves_complete_snapshots_and_replaces_only_own_unchanged_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.json");
        let mut store = SessionStore::new(&path, directory.path()).unwrap();
        assert!(!path.exists());
        let mut messages = vec![
            AgentMessage::User("hello".into()),
            AgentMessage::Assistant("hi".into()),
        ];
        store.save(messages.clone()).unwrap();
        messages.push(AgentMessage::User("1".into()));
        messages.push(AgentMessage::Assistant("confirmed".into()));
        store.save(messages.clone()).unwrap();
        let saved: SessionFile = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved.version, 1);
        assert_eq!(saved.workspace, directory.path());
        assert_eq!(saved.messages, messages);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        // An external change must survive a failed subsequent save.
        std::fs::write(&path, "external modification").unwrap();
        assert!(store.save(messages).is_err());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "external modification"
        );
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn refuses_existing_targets_and_first_save_collisions() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.json");
        let mut store = SessionStore::new(&path, directory.path()).unwrap();
        std::fs::write(&path, "keep me").unwrap();
        assert!(store.save(vec![]).is_err());
        assert!(SessionStore::new(&path, directory.path()).is_err());
        assert!(SessionStore::new(directory.path(), directory.path()).is_err());
        assert!(
            SessionStore::new(
                &directory.path().join("missing/session.json"),
                directory.path()
            )
            .is_err()
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "keep me");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlink_targets_and_retargeted_saved_files() {
        use std::os::unix::fs::symlink;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.json");
        let outside = directory.path().join("other.json");
        std::fs::write(&outside, "untouched").unwrap();
        let mut store = SessionStore::new(&path, directory.path()).unwrap();
        store.save(vec![]).unwrap();
        std::fs::remove_file(&path).unwrap();
        symlink(&outside, &path).unwrap();
        assert!(store.save(vec![]).is_err());
        assert!(SessionStore::new(&path, directory.path()).is_err());
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "untouched");
        std::fs::remove_file(&path).unwrap();
        symlink(directory.path().join("missing"), &path).unwrap();
        assert!(SessionStore::new(&path, directory.path()).is_err());
    }

    #[test]
    fn round_trips_complete_history_including_tool_errors() {
        let calls = vec![
            ToolCall {
                id: "call-1".into(),
                name: "read_file".into(),
                input: r#"{"path":"src/lib.rs"}"#.into(),
            },
            ToolCall {
                id: "call-2".into(),
                name: "read_file".into(),
                input: r#"{"path":"missing.rs"}"#.into(),
            },
        ];
        let snapshot = SessionFile {
            version: 1,
            workspace: std::env::current_dir().unwrap(),
            messages: vec![
                AgentMessage::User("读取文件，包含中文与\"引号\"".into()),
                AgentMessage::AssistantToolCalls(calls),
                AgentMessage::ToolResult {
                    tool_call_id: "call-1".into(),
                    name: "read_file".into(),
                    output: "fn main() {\n    println!(\"你好\");\n}\n".into(),
                    is_error: false,
                },
                AgentMessage::ToolResult {
                    tool_call_id: "call-2".into(),
                    name: "read_file".into(),
                    output: "file does not exist".into(),
                    is_error: true,
                },
                AgentMessage::Assistant("1. 修改文件\n2. 保持不变".into()),
            ],
        };
        let encoded = serde_json::to_string_pretty(&snapshot).unwrap();
        let decoded: SessionFile = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, snapshot);
        let value: serde_json::Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(value["messages"][0]["type"], "user");
        assert_eq!(value["messages"][1]["type"], "assistant_tool_calls");
        assert_eq!(value["messages"][2]["type"], "tool_result");
        assert_eq!(value["messages"][4]["type"], "assistant");
        let fields = value.as_object().unwrap();
        assert_eq!(fields.len(), 3);
        for field in ["version", "workspace", "messages"] {
            assert!(fields.contains_key(field));
        }
    }

    #[test]
    fn round_trips_empty_history() {
        let snapshot = SessionFile {
            version: 1,
            workspace: std::env::current_dir().unwrap(),
            messages: vec![],
        };
        let value = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(
            serde_json::from_value::<SessionFile>(value).unwrap(),
            snapshot
        );
    }

    #[test]
    fn rejects_malformed_snapshots() {
        let valid = json!({"version":1, "workspace":"project", "messages":[]});
        for field in ["version", "workspace", "messages"] {
            let mut missing = valid.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert!(serde_json::from_value::<SessionFile>(missing).is_err());
        }
        let mut invalid = valid.clone();
        invalid["messages"] = json!([{"type":"unknown", "data":"hello"}]);
        assert!(serde_json::from_value::<SessionFile>(invalid).is_err());
        let mut invalid = valid.clone();
        invalid["messages"] = json!([{"type":"tool_result", "data":{"output":"missing call id"}}]);
        assert!(serde_json::from_value::<SessionFile>(invalid).is_err());
        let mut invalid = valid;
        invalid["api_key"] = json!("must not be part of this format");
        assert!(serde_json::from_value::<SessionFile>(invalid).is_err());
    }
}
