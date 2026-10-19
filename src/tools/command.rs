use crate::{agent::ToolOutput, workspace::Workspace};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::{
    path::Path,
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
};

const MAX_OUTPUT_BYTES: usize = 32 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CommandInput {
    program: String,
    args: Vec<String>,
    cwd: Option<String>,
    timeout_seconds: Option<u64>,
}

#[derive(Default)]
struct OutputBuffer {
    bytes: Vec<u8>,
    truncated: bool,
}

impl OutputBuffer {
    fn text(&mut self) -> String {
        let mut text = String::from_utf8_lossy(&self.bytes).into_owned();
        if text.len() > MAX_OUTPUT_BYTES {
            let mut end = MAX_OUTPUT_BYTES;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
            self.truncated = true;
        }
        text
    }
}

async fn drain(
    reader: &mut (impl AsyncRead + Unpin),
    output: &mut OutputBuffer,
) -> std::io::Result<()> {
    let mut buffer = [0; 8192];
    loop {
        let length = reader.read(&mut buffer).await?;
        if length == 0 {
            return Ok(());
        }
        let retained = length.min(MAX_OUTPUT_BYTES - output.bytes.len());
        output.bytes.extend_from_slice(&buffer[..retained]);
        output.truncated |= retained < length;
    }
}

// Drop runs when the surrounding Agent Future is cancelled, including Ctrl+C.
// A separate Unix process group keeps group termination away from our CLI.
struct ProcessGroup {
    #[cfg(unix)]
    id: i32,
}

impl ProcessGroup {
    fn terminate(&self) {
        #[cfg(unix)]
        // SAFETY: the positive PID from our child is also its process group ID.
        // Negating it targets only that group; SIGKILL takes no pointers.
        unsafe {
            libc::kill(-self.id, libc::SIGKILL);
        }
    }
}

impl Drop for ProcessGroup {
    fn drop(&mut self) {
        self.terminate();
    }
}

pub(super) async fn execute(workspace: &Workspace, input: &str) -> Result<ToolOutput> {
    let input: CommandInput =
        serde_json::from_str(input).context("invalid run_command tool input")?;
    if input.program.trim().is_empty()
        || input.program.len() > 1024
        || input.args.len() > 128
        || input.args.iter().map(String::len).sum::<usize>() > 64 * 1024
        || input.program.contains('\0')
        || input.args.iter().any(|arg| arg.contains('\0'))
    {
        bail!(
            "invalid program or arguments (program <= 1024 bytes, <= 128 args, total args <= 64 KiB, no NUL)"
        );
    }
    let seconds = input.timeout_seconds.unwrap_or(60);
    if !(1..=300).contains(&seconds) {
        bail!("timeout_seconds must be between 1 and 300");
    }
    let cwd = input.cwd.as_deref().unwrap_or(".");
    if cwd.trim().is_empty() {
        bail!("cwd must not be empty");
    }
    let cwd = workspace.resolve_existing(Path::new(cwd))?;
    if !cwd.is_dir() {
        bail!("cwd must be a directory");
    }
    let program = Path::new(&input.program);
    let program = if program.is_relative() && program.components().count() > 1 {
        cwd.join(program)
    } else {
        program.to_path_buf()
    };
    let mut command = Command::new(program);
    command
        .args(&input.args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        // Do not pass the CLI's known model credentials to development commands.
        .env_remove("AGENT_CLI_LLM_API_KEY")
        .env_remove("OPENAI_API_KEY");
    #[cfg(unix)]
    command.process_group(0);
    let started = Instant::now();
    let mut child = command.spawn().context("failed to start command")?;
    let _group = ProcessGroup {
        #[cfg(unix)]
        id: i32::try_from(child.id().context("command has no process ID")?)
            .context("command process ID out of range")?,
    };
    let mut stdout = child.stdout.take().context("command stdout unavailable")?;
    let mut stderr = child.stderr.take().context("command stderr unavailable")?;
    let mut out = OutputBuffer::default();
    let mut err = OutputBuffer::default();
    // The deadline covers both process exit and pipe EOF: descendants may keep
    // pipes open after their parent exits. Never stop draining at the size cap.
    let completed = tokio::time::timeout(Duration::from_secs(seconds), async {
        tokio::try_join!(
            child.wait(),
            drain(&mut stdout, &mut out),
            drain(&mut stderr, &mut err)
        )
    })
    .await;
    let (status, timed_out) = match completed {
        Ok(result) => (
            Some(
                result
                    .context("failed to wait for command or read output")?
                    .0,
            ),
            false,
        ),
        Err(_) => {
            _group.terminate();
            let _ = child.start_kill();
            child
                .wait()
                .await
                .context("failed to reap timed-out command")?;
            (None, true)
        }
    };
    let success = !timed_out && status.is_some_and(|status| status.success());
    let stdout = out.text();
    let stderr = err.text();
    let result = serde_json::json!({
        "success":success,"exit_code":status.and_then(|status|status.code()),
        "timed_out":timed_out,"stdout":stdout,"stderr":stderr,
        "stdout_truncated":out.truncated,"stderr_truncated":err.truncated,
        "duration_ms":started.elapsed().as_millis()
    })
    .to_string();
    Ok(if success {
        ToolOutput::success(result)
    } else {
        ToolOutput::error(result)
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::{agent::ToolExecutor, tools::BuiltinToolExecutor};
    use std::fs;

    fn request(program: &str, args: &[&str]) -> String {
        serde_json::json!({"program":program,"args":args}).to_string()
    }

    fn value(output: &ToolOutput) -> serde_json::Value {
        serde_json::from_str(&output.content).unwrap()
    }

    #[tokio::test]
    async fn executes_literal_arguments_with_closed_stdin_and_workspace_cwd() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir(directory.path().join("src")).unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();
        let result = execute(
            &workspace,
            &request("/usr/bin/printf", &["%s", "a && echo injected > file"]),
        )
        .await
        .unwrap();
        assert!(!result.is_error);
        assert_eq!(value(&result)["stdout"], "a && echo injected > file");
        assert!(!directory.path().join("file").exists());
        let result = execute(
            &workspace,
            r#"{"program":"/bin/pwd","args":[],"cwd":"src"}"#,
        )
        .await
        .unwrap();
        assert_eq!(
            value(&result)["stdout"].as_str().unwrap().trim(),
            directory
                .path()
                .join("src")
                .canonicalize()
                .unwrap()
                .to_str()
                .unwrap()
        );
        let result = execute(&workspace, &request("/bin/cat", &[]))
            .await
            .unwrap();
        assert_eq!(value(&result)["stdout"], "");
        assert_eq!(value(&result)["exit_code"], 0);
    }

    #[tokio::test]
    async fn returns_nonzero_exit_and_drains_both_pipes_past_limits() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();
        let result = execute(
            &workspace,
            &request("/bin/sh", &["-c", "printf out; printf err >&2; exit 7"]),
        )
        .await
        .unwrap();
        assert!(result.is_error);
        assert_eq!(value(&result)["exit_code"], 7);
        assert_eq!(value(&result)["stdout"], "out");
        assert_eq!(value(&result)["stderr"], "err");
        let result = execute(&workspace,&request("/bin/sh",&["-c","i=0; while [ $i -lt 5000 ]; do printf 0123456789; printf 0123456789 >&2; i=$((i+1)); done"])).await.unwrap();
        assert!(!result.is_error);
        let output = value(&result);
        for stream in ["stdout", "stderr"] {
            assert_eq!(output[stream].as_str().unwrap().len(), MAX_OUTPUT_BYTES);
            assert_eq!(output[format!("{stream}_truncated")], true);
        }
    }

    #[tokio::test]
    async fn rejects_invalid_commands_inputs_and_working_directories() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("file"), "").unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();
        for input in [
            serde_json::json!({"program":"","args":[]}),
            serde_json::json!({"program":"/bin/pwd","args":[],"timeout_seconds":0}),
            serde_json::json!({"program":"/bin/pwd","args":[],"timeout_seconds":301}),
            serde_json::json!({"program":"/bin/pwd","args":[],"cwd":".."}),
            serde_json::json!({"program":"/bin/pwd","args":[],"cwd":"file"}),
            serde_json::json!({"program":"/bin/pwd","args":[],"cwd":"missing"}),
            serde_json::json!({"program":"/bin/pwd","args":["\0"]}),
        ] {
            assert!(
                execute(&workspace, &input.to_string()).await.is_err(),
                "{input}"
            );
        }
        let error = execute(&workspace, &request("/does-not-exist-claw-test", &[]))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("failed to start command"));
    }

    #[tokio::test]
    async fn executor_requires_exec_permission_independently_of_write_access() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();
        let mut executor = BuiltinToolExecutor::new(workspace).with_write_access(true);
        let denied = executor
            .execute("run_command", "invalid JSON")
            .await
            .unwrap();
        assert!(denied.is_error);
        assert!(denied.content.contains("execution access denied"));
        let mut executor = executor.with_write_access(false).with_exec_access(true);
        let output = executor
            .execute("run_command", &request("/bin/sh", &["-c", "exit 4"]))
            .await
            .unwrap();
        assert!(output.is_error);
        assert_eq!(value(&output)["exit_code"], 4);
    }

    #[tokio::test]
    async fn timeout_kills_group_even_if_parent_exited_with_pipes_open() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();
        let result = execute(&workspace,&serde_json::json!({
            "program":"/bin/sh", "args":["-c","(sleep 2; printf orphan > late) & exit 0"],"timeout_seconds":1
        }).to_string()).await.unwrap();
        assert!(result.is_error);
        assert_eq!(value(&result)["timed_out"], true);
        assert_eq!(value(&result)["exit_code"], serde_json::Value::Null);
        tokio::time::sleep(Duration::from_millis(1300)).await;
        assert!(!directory.path().join("late").exists());
    }

    #[tokio::test]
    async fn cancelling_future_kills_child_group() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();
        let task = tokio::spawn(async move {
            execute(
                &workspace,
                &request(
                    "/bin/sh",
                    &[
                        "-c",
                        "(sleep 2; printf orphan > late) & printf ready > ready; wait",
                    ],
                ),
            )
            .await
        });
        tokio::time::timeout(Duration::from_secs(5), async {
            while !directory.path().join("ready").exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        tokio::time::sleep(Duration::from_millis(2200)).await;
        assert!(!directory.path().join("late").exists());
    }
}
