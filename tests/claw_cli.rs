use std::io::Write;

use assert_cmd::Command;
use predicates::str::{contains, is_empty};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_json, body_partial_json, header, method, path},
};

fn write_temp_config(contents: &str) -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(contents.as_bytes()).unwrap();
    file
}

#[tokio::test]
async fn claw_only_exposes_and_executes_edits_with_write_authorization() {
    for allow_write in [false, true] {
        let server = MockServer::start().await;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("file");
        std::fs::write(&path, "old\n").unwrap();
        let config = write_temp_config(&format!(
            "[llm]\nbase_url = {:?}\napi_key = \"test-key\"\nmodel = \"test-model\"\n",
            server.uri()
        ));
        Mock::given(method("POST"))
            .and(move |request: &wiremock::Request| {
                let body: serde_json::Value = request.body_json().unwrap();
                let edit_tool = body["tools"].as_array().unwrap().iter()
                    .any(|tool| tool["function"]["name"] == "replace_text");
                body["messages"].as_array().unwrap().len() == 1 && edit_tool == allow_write
            })
            // Also send a forged call when the schema is hidden: executor
            // authorization must stand independently of model behavior.
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices":[{"message":{"content":null,"tool_calls":[{
                    "id":"edit-1","type":"function","function":{
                        "name":"replace_text","arguments":"{\"path\":\"file\",\"old_text\":\"old\",\"new_text\":\"new\"}"
                    }
                }]}}]
            }))).expect(1).mount(&server).await;
        Mock::given(method("POST"))
            .and(move |request: &wiremock::Request| {
                let body: serde_json::Value = request.body_json().unwrap();
                let message = &body["messages"][2];
                if message["role"] != "tool" || message["tool_call_id"] != "edit-1" {
                    return false;
                }
                let Some(result) = message["content"]
                    .as_str()
                    .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
                else {
                    return false;
                };
                if allow_write {
                    result["applied"] == true
                        && result["path"] == "file"
                        && result["diff"]
                            .as_str()
                            .is_some_and(|diff| diff.contains("-old\n+new\n"))
                } else {
                    result["error"]
                        .as_str()
                        .is_some_and(|error| error.contains("write access denied"))
                }
            })
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices":[{"message":{"content":"finished"}}]
            })))
            .expect(1)
            .mount(&server)
            .await;
        let mut command = Command::cargo_bin("agent-cli-rust").unwrap();
        command
            .arg("claw")
            .arg(config.path())
            .arg("edit file")
            .arg("--workspace")
            .arg(directory.path())
            .args(["--max-iterations", "2"]);
        if allow_write {
            command.arg("--allow-write");
        }
        command
            .timeout(std::time::Duration::from_secs(5))
            .assert()
            .success()
            .stdout(contains("finished"))
            .stderr(is_empty());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            if allow_write { "new\n" } else { "old\n" }
        );
    }
}

fn list_files_tool_definition() -> serde_json::Value {
    serde_json::json!({
        "type": "function",
        "function": {
            "name": "list_files",
            "description": "List direct children of a workspace directory, sorted by path. Returns at most 200 entries and a truncated flag. Paths are workspace-relative; use '.' for the root. Symlinks are listed without following them.",
            "parameters": {
                "type":"object", "properties":{"path":{"type":"string","minLength":1}},
                "required":["path"], "additionalProperties":false
            }
        }
    })
}

fn search_code_tool_definition() -> serde_json::Value {
    serde_json::json!({
        "type":"function",
        "function": {
            "name":"search_code",
            "description":"Search a workspace file or directory recursively for a case-sensitive literal single-line pattern (1-1024 bytes). Skip discovered symlinks, .git and target directories, binary/non-UTF-8 files, and files over 1 MiB. Scan at most 5000 directory entries and 8 MiB. Return up to 100 matching lines sorted by path and line, within 32 KiB JSON. Each text excerpt is at most 1024 bytes and may start at the match; line_truncated marks shortened lines. truncated marks incomplete scanning/results; skipped_files counts unsupported files. No matches is a successful result. Does not apply .gitignore rules.",
            "parameters": {
                "type":"object",
                "properties": {
                    "pattern":{"type":"string","minLength":1,"maxLength":1024},
                    "path":{"type":"string","minLength":1}
                },
                "required":["pattern","path"], "additionalProperties":false
            }
        }
    })
}

fn read_file_tool_definition() -> serde_json::Value {
    serde_json::json!({
        "type":"function",
        "function": {
            "name":"read_file",
            "description":"Read a workspace UTF-8 regular file up to 1 MiB. Lines are 1-based and inclusive; defaults to 200 lines from start_line (default 1). Returns at most 500 lines and 32 KiB of content. truncated means content remains after the returned text, including when end_line was requested; the last line may be partial at the byte limit. Empty files return end_line 0.",
            "parameters": {
                "type":"object",
                "properties": {
                    "path":{"type":"string","minLength":1},
                    "start_line":{"type":"integer","minimum":1},
                    "end_line":{"type":"integer","minimum":1}
                },
                "required":["path"], "additionalProperties":false
            }
        }
    })
}

#[tokio::test]
async fn claw_lists_workspace_and_returns_answer() {
    let server = MockServer::start().await;
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("src")).unwrap();
    let config = write_temp_config(&format!(
        "[llm]\nbase_url = {:?}\napi_key = \"test-key\"\nmodel = \"test-model\"\n",
        server.uri()
    ));
    Mock::given(method("POST"))
        .and(body_partial_json(serde_json::json!({
            "messages":[{"role":"user","content":"list project"}]
        })))
        .and(|request: &wiremock::Request| {
            request.body_json::<serde_json::Value>().unwrap()["messages"]
                .as_array()
                .is_some_and(|messages| messages.len() == 1)
        })
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices":[{"message":{"content":null,"tool_calls":[{
                "id":"list-1","type":"function",
                "function":{"name":"list_files","arguments":"{\"path\":\".\"}"}
            }]}}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let tool_content =
        serde_json::json!({"entries":[{"path":"src","type":"directory"}],"truncated":false})
            .to_string();
    Mock::given(method("POST"))
        .and(body_partial_json(serde_json::json!({
            "messages":[
                {"role":"user","content":"list project"},
                {"role":"assistant","content":null,"tool_calls":[{
                    "id":"list-1","type":"function","function":{"name":"list_files","arguments":"{\"path\":\".\"}"}
                }]},
                {"role":"tool","tool_call_id":"list-1","content":tool_content}
            ]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices":[{"message":{"content":"The project contains src."}}]
        })))
        .expect(1).mount(&server).await;
    Command::cargo_bin("agent-cli-rust")
        .unwrap()
        .arg("claw")
        .arg(config.path())
        .arg("list project")
        .arg("--workspace")
        .arg(directory.path())
        .args(["--max-iterations", "2"])
        .timeout(std::time::Duration::from_secs(5))
        .assert()
        .success()
        .stdout(contains("The project contains src."))
        .stderr(is_empty());
}

#[tokio::test]
async fn claw_reads_file_and_returns_answer() {
    let server = MockServer::start().await;
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("main.rs"), "fn main() {}\n").unwrap();
    let config = write_temp_config(&format!(
        "[llm]\nbase_url = {:?}\napi_key = \"test-key\"\nmodel = \"test-model\"\n",
        server.uri()
    ));
    Mock::given(method("POST"))
        .and(body_partial_json(serde_json::json!({
            "messages":[{"role":"user","content":"read project"}]
        })))
        .and(|request: &wiremock::Request| {
            request.body_json::<serde_json::Value>().unwrap()["messages"]
                .as_array()
                .is_some_and(|messages| messages.len() == 1)
        })
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices":[{"message":{"content":null,"tool_calls":[{
                "id":"read-1","type":"function",
                "function":{"name":"read_file","arguments":"{\"path\":\"main.rs\"}"}
            }]}}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let tool_content =
        serde_json::json!({"path":"main.rs","start_line":1,"end_line":1,"content":"fn main() {}\n","truncated":false})
            .to_string();
    Mock::given(method("POST"))
        .and(body_partial_json(serde_json::json!({
            "messages":[
                {"role":"user","content":"read project"},
                {"role":"assistant","content":null,"tool_calls":[{
                    "id":"read-1","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"main.rs\"}"}
                }]},
                {"role":"tool","tool_call_id":"read-1","content":tool_content}
            ]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices":[{"message":{"content":"The file defines main."}}]
        })))
        .expect(1).mount(&server).await;
    Command::cargo_bin("agent-cli-rust")
        .unwrap()
        .arg("claw")
        .arg(config.path())
        .arg("read project")
        .arg("--workspace")
        .arg(directory.path())
        .args(["--max-iterations", "2"])
        .timeout(std::time::Duration::from_secs(5))
        .assert()
        .success()
        .stdout(contains("The file defines main."))
        .stderr(is_empty());
}

#[tokio::test]
async fn claw_searches_code_and_returns_answer() {
    let server = MockServer::start().await;
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("main.rs"), "fn main() {}\n").unwrap();
    let config = write_temp_config(&format!(
        "[llm]\nbase_url = {:?}\napi_key = \"test-key\"\nmodel = \"test-model\"\n",
        server.uri()
    ));
    Mock::given(method("POST"))
        .and(body_partial_json(serde_json::json!({
            "messages":[{"role":"user","content":"search project"}]
        })))
        .and(|request: &wiremock::Request| {
            request.body_json::<serde_json::Value>().unwrap()["messages"]
                .as_array()
                .is_some_and(|messages| messages.len() == 1)
        })
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices":[{"message":{"content":null,"tool_calls":[{
                "id":"search-1","type":"function",
                "function":{"name":"search_code","arguments":"{\"pattern\":\"fn main\",\"path\":\".\"}"}
            }]}}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let tool_content = serde_json::json!({"matches":[{"path":"main.rs","line":1,"text":"fn main() {}","line_truncated":false}],"truncated":false,"skipped_files":0});
    Mock::given(method("POST"))
        .and(body_partial_json(serde_json::json!({
            "messages":[
                {"role":"user","content":"search project"},
                {"role":"assistant","content":null,"tool_calls":[{
                    "id":"search-1","type":"function","function":{"name":"search_code","arguments":"{\"pattern\":\"fn main\",\"path\":\".\"}"}
                }]},
                {"role":"tool","tool_call_id":"search-1"}
            ]
        })))
        .and(move |request: &wiremock::Request| {
            let body: serde_json::Value = request.body_json().unwrap();
            body["messages"][2]["content"].as_str()
                .and_then(|content| serde_json::from_str::<serde_json::Value>(content).ok())
                .is_some_and(|content| content == tool_content)
        })
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices":[{"message":{"content":"Found main in main.rs at line 1."}}]
        })))
        .expect(1).mount(&server).await;
    Command::cargo_bin("agent-cli-rust")
        .unwrap()
        .arg("claw")
        .arg(config.path())
        .arg("search project")
        .arg("--workspace")
        .arg(directory.path())
        .args(["--max-iterations", "2"])
        .timeout(std::time::Duration::from_secs(5))
        .assert()
        .success()
        .stdout(contains("Found main in main.rs at line 1."))
        .stderr(is_empty());
}

#[tokio::test]
async fn claw_rejects_invalid_workspace_before_requesting_model() {
    let server = MockServer::start().await;
    let config = write_temp_config(&format!(
        "[llm]\nbase_url = {:?}\napi_key = \"test-key\"\nmodel = \"test-model\"\n",
        server.uri()
    ));
    let directory = tempfile::tempdir().unwrap();
    let missing = directory.path().join("missing");
    for (workspace, error) in [
        (missing.as_path(), "failed to resolve workspace"),
        (config.path(), "workspace must be a directory"),
    ] {
        Command::cargo_bin("agent-cli-rust")
            .unwrap()
            .arg("claw")
            .arg(config.path())
            .args(["hello", "--workspace"])
            .arg(workspace)
            .assert()
            .code(1)
            .stderr(contains(error))
            .stdout(is_empty());
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[test]
fn claw_rejects_zero_budgets() {
    for flag in ["--max-iterations", "--max-tool-calls"] {
        Command::cargo_bin("agent-cli-rust")
            .unwrap()
            .args(["claw", "unused.toml", "hello", flag, "0"])
            .assert()
            .code(2)
            .stderr(contains("must be greater than 0"))
            .stdout(is_empty());
    }
}

#[tokio::test]
async fn claw_passes_explicit_budgets_to_runtime() {
    for (flag, expected_requests, error) in [
        ("--max-iterations", 1, "agent reached maximum iterations"),
        ("--max-tool-calls", 2, "agent reached maximum tool calls"),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {
                    "content": null,
                    "tool_calls": [{
                        "id": "call-1",
                        "type": "function",
                        "function": {"name": "add", "arguments": "{\"a\":2,\"b\":3}"}
                    }]
                }}]
            })))
            .expect(expected_requests)
            .mount(&server)
            .await;
        let config = write_temp_config(&format!(
            "[llm]\nbase_url = {:?}\napi_key = \"test-key\"\nmodel = \"test-model\"\n",
            server.uri()
        ));
        Command::cargo_bin("agent-cli-rust")
            .unwrap()
            .arg("claw")
            .arg(config.path())
            .args(["hello", flag, "1"])
            .timeout(std::time::Duration::from_secs(5))
            .assert()
            .code(1)
            .stderr(contains(error))
            .stdout(is_empty());
    }
}

#[cfg(unix)]
#[tokio::test]
async fn claw_cancels_pending_model_request_on_sigint() {
    use std::process::{Command as ProcessCommand, Stdio};
    use std::time::Duration;

    // Ensure a failed assertion cannot leave the test child running.
    struct ChildGuard(std::process::Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(30))
                .set_body_json(serde_json::json!({
                    "choices": [{"message": {"content": "late answer"}}]
                })),
        )
        .mount(&server)
        .await;

    let config_file = write_temp_config(&format!(
        "[llm]\nbase_url = {:?}\napi_key = \"test-key\"\nmodel = \"test-model\"\n",
        server.uri()
    ));
    let mut child = ChildGuard(
        ProcessCommand::new(assert_cmd::cargo::cargo_bin("agent-cli-rust"))
            .arg("claw")
            .arg(config_file.path())
            .arg("hello")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );

    // Synchronize on the real request rather than guessing startup duration.
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if !server.received_requests().await.unwrap().is_empty() {
                break;
            }
            assert!(child.0.try_wait().unwrap().is_none());
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("CLI did not send its model request");

    assert!(
        ProcessCommand::new("/bin/kill")
            .args(["-INT", &child.0.id().to_string()])
            .status()
            .unwrap()
            .success()
    );

    let status = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                break status;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("CLI did not exit after SIGINT");

    use std::io::Read;
    let mut stdout = String::new();
    let mut stderr = String::new();
    child
        .0
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut stdout)
        .unwrap();
    child
        .0
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    assert_eq!(status.code(), Some(130));
    assert!(stdout.is_empty(), "unexpected stdout: {stdout}");
    assert_eq!(stderr.trim(), "claw task cancelled");
}

#[tokio::test]
async fn claw_prints_final_answer_from_mock_server() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("authorization", "Bearer test-key"))
        .and(body_partial_json(serde_json::json!({
            "model": "test-model",
            "messages": [{
                "role": "user",
                "content": "hello"
            }],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "add"
                }
            }]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "message": {
                    "content": "Hello from Claw"
                }
            }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let config_file = write_temp_config(&format!(
        r#"[llm]
base_url = "{}"
api_key = "test-key"
model = "test-model"
temperature = 0.2
"#,
        server.uri()
    ));

    let mut command = Command::cargo_bin("agent-cli-rust").unwrap();

    command
        .arg("claw")
        .arg(config_file.path())
        .arg("hello")
        .arg("--workspace")
        .arg(config_file.path().parent().unwrap())
        .assert()
        .success()
        .stdout(contains("Hello from Claw"))
        .stderr(is_empty());
}

#[tokio::test]
async fn claw_executes_tool_and_prints_final_answer() {
    let server = MockServer::start().await;
    let add_tool = serde_json::json!({
        "type": "function",
        "function": {
            "name": "add",
            "description": "Add two integers",
            "parameters": {
                "type": "object",
                "properties": {
                    "a": {
                        "type": "integer"
                    },
                    "b": {
                        "type": "integer"
                    }
                },
                "required": ["a", "b"],
                "additionalProperties": false
            }
        }
    });

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("authorization", "Bearer test-key"))
        .and(body_json(serde_json::json!({
            "model": "test-model",
            "temperature": 0.2,
            "messages": [{
                "role": "user",
                "content": "add 2 and 3"
            }],
            "stream": false,
            "tools": [add_tool.clone(), list_files_tool_definition(), read_file_tool_definition(), search_code_tool_definition()]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "message": {
                    "content": null,
                    "tool_calls": [{
                        "id": "call-1",
                        "type": "function",
                        "function": {
                            "name": "add",
                            "arguments": "{\"a\":2,\"b\":3}"
                        }
                    }]
                }
            }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("authorization", "Bearer test-key"))
        .and(body_json(serde_json::json!({
            "model": "test-model",
            "temperature": 0.2,
            "messages": [
                {
                    "role": "user",
                    "content": "add 2 and 3"
                },
                {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call-1",
                        "type": "function",
                        "function": {
                            "name": "add",
                            "arguments": "{\"a\":2,\"b\":3}"
                        }
                    }]
                },
                {
                    "role": "tool",
                    "tool_call_id": "call-1",
                    "content": "5"
                }
            ],
            "stream": false,
            "tools": [add_tool, list_files_tool_definition(), read_file_tool_definition(), search_code_tool_definition()]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "message": {
                    "content": "The result is 5"
                }
            }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let config_file = write_temp_config(&format!(
        r#"[llm]
base_url = "{}"
api_key = "test-key"
model = "test-model"
temperature = 0.2
"#,
        server.uri()
    ));

    let mut command = Command::cargo_bin("agent-cli-rust").unwrap();

    command
        .arg("claw")
        .arg(config_file.path())
        .arg("add 2 and 3")
        .assert()
        .success()
        .stdout(contains("The result is 5"))
        .stderr(is_empty());
}
