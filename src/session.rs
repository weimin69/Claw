//! Local session file format, separate from provider-specific HTTP messages.
//! Histories may contain source code and sensitive tool output. This format
//! intentionally has no credentials or execution authorization fields.
use anyhow::{Context, Result, bail};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::agent::{AgentMessage, AgentSession, validate_history};

const MAX_SESSION_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SessionFile {
    pub(crate) version: u32,
    /// Canonical workspace path, checked against the current project on restore.
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

/// Return the exact bytes read as well as the validated snapshot, so subsequent
/// writes compare against what was actually loaded, not a second filesystem read.
pub(crate) fn load_session(path: &Path, workspace: &Path) -> Result<(SessionFile, Vec<u8>)> {
    let file = std::fs::File::open(path).context("failed to open session file")?;
    let metadata = file.metadata().context("failed to inspect session file")?;
    if !metadata.is_file() {
        bail!("session path must be a regular file");
    }

    if metadata.len() > MAX_SESSION_BYTES {
        bail!("session file exceeds size limit");
    }

    let mut bytes = Vec::new();
    file.take(MAX_SESSION_BYTES + 1)
        .read_to_end(&mut bytes)
        .context("failed to read session file")?;

    if bytes.len() as u64 > MAX_SESSION_BYTES {
        bail!("session file exceeds size limit");
    }

    let mut snapshot: SessionFile =
        serde_json::from_slice(&bytes).context("failed to parse session file")?;

    if snapshot.version != 1 {
        bail!("unsupported session version: {}", snapshot.version);
    }

    let saved_workspace = snapshot
        .workspace
        .canonicalize()
        .context("failed to resolve saved workspace")?;

    let current_workspace = workspace
        .canonicalize()
        .context("failed to resolve current workspace")?;

    if !current_workspace.is_dir() {
        bail!("session workspace must be a directory");
    }

    if saved_workspace != current_workspace {
        bail!("session workspace does not match current workspace");
    }

    snapshot.workspace = saved_workspace;
    validate_history(&snapshot.messages)?;
    Ok((snapshot, bytes))
}

impl SessionStore {
    pub(crate) fn resume(path: &Path, workspace: &Path) -> Result<(Self, AgentSession)> {
        if !std::fs::symlink_metadata(path)
            .context("failed to inspect resume file")?
            .file_type()
            .is_file()
        {
            bail!("resume path must be a regular file, not a symlink");
        }
        let path = path
            .canonicalize()
            .context("failed to resolve resume file")?;
        let (snapshot, bytes) = load_session(&path, workspace)?;
        let session = AgentSession::from_history(snapshot.messages)?;
        Ok((
            Self {
                path,
                workspace: snapshot.workspace,
                last_saved: Some(bytes),
            },
            session,
        ))
    }
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
        if bytes.len() as u64 > MAX_SESSION_BYTES {
            bail!("session file exceeds size limit");
        }
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

    fn write_snapshot(path: &Path, workspace: &Path, version: u32) -> SessionFile {
        let snapshot = SessionFile {
            version,
            workspace: workspace.to_path_buf(),
            messages: vec![
                AgentMessage::User("你好".into()),
                AgentMessage::Assistant("hello".into()),
            ],
        };
        std::fs::write(path, serde_json::to_vec(&snapshot).unwrap()).unwrap();
        snapshot
    }

    #[test]
    fn resumed_store_updates_original_and_preserves_external_changes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.json");
        let snapshot = write_snapshot(&path, directory.path(), 1);
        let original = std::fs::read(&path).unwrap();
        let (mut store, session) = SessionStore::resume(&path, directory.path()).unwrap();
        assert_eq!(session.completed_messages().unwrap(), snapshot.messages);
        assert_eq!(store.last_saved.as_ref().unwrap(), &original);
        let mut messages = snapshot.messages.clone();
        messages.extend([
            AgentMessage::User("1".into()),
            AgentMessage::Assistant("done".into()),
        ]);
        store.save(messages.clone()).unwrap();
        assert_eq!(
            load_session(&path, directory.path()).unwrap().0.messages,
            messages
        );
        std::fs::write(&path, "external").unwrap();
        assert!(store.save(messages).is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "external");
    }

    #[test]
    fn rejects_incomplete_or_mispaired_history() {
        let user = AgentMessage::User("hello".into());
        let answer = AgentMessage::Assistant("done".into());
        let call = ToolCall {
            id: "1".into(),
            name: "read_file".into(),
            input: "{}".into(),
        };
        let batch = AgentMessage::AssistantToolCalls(vec![call.clone()]);
        let result = AgentMessage::ToolResult {
            tool_call_id: "1".into(),
            name: "read_file".into(),
            output: "text".into(),
            is_error: false,
        };
        let mut wrong_id = result.clone();
        if let AgentMessage::ToolResult { tool_call_id, .. } = &mut wrong_id {
            *tool_call_id = "other".into();
        }
        let mut wrong_name = result.clone();
        if let AgentMessage::ToolResult { name, .. } = &mut wrong_name {
            *name = "other".into();
        }
        let mut blank_call = call.clone();
        blank_call.id = " ".into();
        for messages in [
            vec![],
            vec![answer.clone()],
            vec![user.clone()],
            vec![user.clone(), user.clone(), answer.clone()],
            vec![user.clone(), result.clone(), answer.clone()],
            vec![user.clone(), batch.clone()],
            vec![user.clone(), batch.clone(), result.clone()],
            vec![user.clone(), batch.clone(), answer.clone()],
            vec![user.clone(), batch.clone(), wrong_id, answer.clone()],
            vec![user.clone(), batch.clone(), wrong_name, answer.clone()],
            vec![
                user.clone(),
                AgentMessage::AssistantToolCalls(vec![]),
                answer.clone(),
            ],
            vec![
                user.clone(),
                AgentMessage::AssistantToolCalls(vec![call.clone(), call]),
                result.clone(),
                answer.clone(),
            ],
            vec![
                user.clone(),
                AgentMessage::AssistantToolCalls(vec![blank_call]),
                result.clone(),
                answer.clone(),
            ],
        ] {
            assert!(AgentSession::from_history(messages).is_err());
        }
        assert!(AgentSession::from_history(vec![user, batch, result, answer]).is_ok());
    }

    #[test]
    fn load_rejects_semantically_invalid_history() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.json");
        let mut snapshot = write_snapshot(&path, directory.path(), 1);
        snapshot.messages.pop();
        std::fs::write(&path, serde_json::to_vec(&snapshot).unwrap()).unwrap();
        assert!(
            load_session(&path, directory.path())
                .unwrap_err()
                .to_string()
                .contains("incomplete turn")
        );
    }

    #[test]
    fn loads_snapshot_with_matching_canonical_workspace() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.json");
        let mut expected = write_snapshot(&path, directory.path(), 1);
        expected.workspace = directory.path().canonicalize().unwrap();
        let loaded = load_session(&path, &directory.path().join(".")).unwrap().0;
        assert_eq!(loaded, expected);
    }

    #[test]
    fn rejects_unsupported_version_and_mismatched_or_invalid_workspace() {
        let directory = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.json");
        write_snapshot(&path, directory.path(), 2);
        assert!(
            load_session(&path, directory.path())
                .unwrap_err()
                .to_string()
                .contains("unsupported session version")
        );
        write_snapshot(&path, other.path(), 1);
        assert!(
            load_session(&path, directory.path())
                .unwrap_err()
                .to_string()
                .contains("workspace does not match")
        );
        write_snapshot(&path, &directory.path().join("missing"), 1);
        assert!(
            load_session(&path, directory.path())
                .unwrap_err()
                .to_string()
                .contains("saved workspace")
        );
        write_snapshot(&path, directory.path(), 1);
        assert!(
            load_session(&path, &directory.path().join("missing"))
                .unwrap_err()
                .to_string()
                .contains("current workspace")
        );
        let not_directory = directory.path().join("plain-file");
        std::fs::write(&not_directory, "text").unwrap();
        write_snapshot(&path, &not_directory, 1);
        assert!(
            load_session(&path, &not_directory)
                .unwrap_err()
                .to_string()
                .contains("must be a directory")
        );
    }

    #[test]
    fn rejects_missing_malformed_and_oversized_files() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.json");
        assert!(
            load_session(&path, directory.path())
                .unwrap_err()
                .to_string()
                .contains("failed to open")
        );
        for bytes in [b"not json".as_slice(), b"\xff", b"{}"] {
            std::fs::write(&path, bytes).unwrap();
            assert!(
                load_session(&path, directory.path())
                    .unwrap_err()
                    .to_string()
                    .contains("failed to parse")
            );
        }
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(MAX_SESSION_BYTES + 1).unwrap();
        assert!(
            load_session(&path, directory.path())
                .unwrap_err()
                .to_string()
                .contains("size limit")
        );
    }

    #[test]
    fn accepts_file_exactly_at_size_limit() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.json");
        let mut expected = write_snapshot(&path, directory.path(), 1);
        expected.workspace = directory.path().canonicalize().unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        bytes.resize(MAX_SESSION_BYTES as usize, b' ');
        std::fs::write(&path, bytes).unwrap();
        assert_eq!(load_session(&path, directory.path()).unwrap().0, expected);
    }

    #[cfg(unix)]
    #[test]
    fn checks_opened_target_type_and_allows_symlink_to_regular_file() {
        let directory = tempfile::tempdir().unwrap();
        assert!(
            load_session(directory.path(), directory.path())
                .unwrap_err()
                .to_string()
                .contains("regular file")
        );
        let path = directory.path().join("session.json");
        write_snapshot(&path, directory.path(), 1);
        let link = directory.path().join("link.json");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert_eq!(
            load_session(&link, directory.path()).unwrap(),
            load_session(&path, directory.path()).unwrap()
        );
    }

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
