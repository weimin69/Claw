use assert_cmd::Command;
use predicates::str::contains;
use serde_json::{Value, json};
use std::{io::Write, time::Duration};
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};

fn config(server: &MockServer) -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    write!(
        file,
        "[llm]\nbase_url = {:?}\napi_key = \"private-test-key\"\nmodel = \"mock-model\"\n",
        server.uri()
    )
    .unwrap();
    file
}

#[cfg(unix)]
#[tokio::test]
async fn cancellation_keeps_previous_completed_snapshot() {
    use std::process::Stdio;
    use tokio::io::AsyncWriteExt;
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(|request: &wiremock::Request| {
            let body: Value = request.body_json().unwrap();
            if body["messages"].as_array().unwrap().len() == 1 {
                ResponseTemplate::new(200)
                    .set_body_json(json!({"choices":[{"message":{"content":"completed"}}]}))
            } else {
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_secs(30))
                    .set_body_json(json!({"choices":[{"message":{"content":"too late"}}]}))
            }
        })
        .expect(2)
        .mount(&server)
        .await;
    let config = config(&server);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("history.json");
    let mut child = tokio::process::Command::new(assert_cmd::cargo::cargo_bin("agent-cli-rust"))
        .arg("claw")
        .arg(config.path())
        .arg("--interactive")
        .arg("--session-file")
        .arg(&path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    input.write_all(b"first\nsecond\n").await.unwrap();
    drop(input);
    tokio::time::timeout(Duration::from_secs(5), async {
        while server.received_requests().await.unwrap().len() < 2 {
            assert!(child.try_wait().unwrap().is_none());
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("second request was not sent");
    let before = std::fs::read(&path).unwrap();
    assert!(
        std::process::Command::new("/bin/kill")
            .args(["-INT", &child.id().unwrap().to_string()])
            .status()
            .unwrap()
            .success()
    );
    let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status.code(), Some(130));
    assert_eq!(std::fs::read(path).unwrap(), before);
    let snapshot: Value = serde_json::from_slice(&before).unwrap();
    assert_eq!(snapshot["full_history"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn saves_each_completed_turn_to_explicit_or_automatic_archive() {
    for save in [false, true] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(|request: &wiremock::Request| {
                let body: Value = request.body_json().unwrap();
                let answer = if body["messages"].as_array().unwrap().len() == 1 {
                    "1. choose this"
                } else {
                    "confirmed"
                };
                ResponseTemplate::new(200)
                    .set_body_json(json!({"choices":[{"message":{"content":answer}}]}))
            })
            .expect(2)
            .mount(&server)
            .await;
        let config = config(&server);
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("history.json");
        let mut cmd = Command::cargo_bin("agent-cli-rust").unwrap();
        cmd.arg("claw")
            .arg(config.path())
            .arg("--interactive")
            .arg("--workspace")
            .arg(directory.path());
        if save {
            cmd.arg("--session-file").arg(&path);
        }
        cmd.write_stdin("options\n1\n/exit\n")
            .timeout(Duration::from_secs(10))
            .assert()
            .success()
            .stdout(contains("confirmed"));
        {
            let path = if save {
                path
            } else {
                let archives: Vec<_> =
                    std::fs::read_dir(directory.path().join(".agent-cli/sessions"))
                        .unwrap()
                        .map(|entry| entry.unwrap().path())
                        .collect();
                assert_eq!(archives.len(), 1);
                archives[0].clone()
            };
            let text = std::fs::read_to_string(&path).unwrap();
            let snapshot: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(snapshot["version"], 3);
            assert_eq!(
                snapshot["workspace"],
                directory.path().canonicalize().unwrap().to_str().unwrap()
            );
            assert_eq!(
                snapshot["full_history"],
                json!([
                    {"type":"user","data":"options\n"},
                    {"type":"assistant","data":"1. choose this"},
                    {"type":"user","data":"1\n"},
                    {"type":"assistant","data":"confirmed"}
                ])
            );
            assert!(!text.contains("private-test-key"));
        }
    }
}

#[tokio::test]
async fn failed_turn_preserves_last_completed_snapshot() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(|request: &wiremock::Request| {
            let body: Value = request.body_json().unwrap();
            if body["messages"].as_array().unwrap().len() == 1 {
                ResponseTemplate::new(200)
                    .set_body_json(json!({"choices":[{"message":{"content":"saved answer"}}]}))
            } else {
                ResponseTemplate::new(500)
            }
        })
        .expect(2)
        .mount(&server)
        .await;
    let config = config(&server);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("history.json");
    Command::cargo_bin("agent-cli-rust")
        .unwrap()
        .arg("claw")
        .arg(config.path())
        .arg("--interactive")
        .arg("--session-file")
        .arg(&path)
        .write_stdin("first\nsecond\n")
        .timeout(Duration::from_secs(10))
        .assert()
        .failure()
        .stdout(contains("saved answer"));
    let snapshot: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(snapshot["full_history"].as_array().unwrap().len(), 2);
    assert_eq!(snapshot["full_history"][1]["data"], "saved answer");
}

#[tokio::test]
async fn saves_tool_calls_and_recoverable_results_with_matching_ids() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(|request: &wiremock::Request| {
            let body: Value = request.body_json().unwrap();
            let message = if body["messages"].as_array().unwrap().len() == 1 {
                json!({"content":null,"tool_calls":[{
                    "id":"missing-file", "type":"function",
                    "function":{"name":"read_file","arguments":"{\"path\":\"missing.rs\"}"}
                }]})
            } else {
                json!({"content":"file not found"})
            };
            ResponseTemplate::new(200).set_body_json(json!({"choices":[{"message":message}]}))
        })
        .expect(2)
        .mount(&server)
        .await;
    let config = config(&server);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("history.json");
    Command::cargo_bin("agent-cli-rust")
        .unwrap()
        .arg("claw")
        .arg(config.path())
        .arg("--interactive")
        .arg("--workspace")
        .arg(directory.path())
        .arg("--session-file")
        .arg(&path)
        .write_stdin("read missing file\n/exit\n")
        .timeout(Duration::from_secs(10))
        .assert()
        .success();
    let snapshot: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let messages = snapshot["full_history"].as_array().unwrap();
    assert_eq!(messages.len(), 4);
    assert_eq!(messages[1]["type"], "assistant_tool_calls");
    assert_eq!(messages[1]["data"][0]["id"], "missing-file");
    assert_eq!(messages[1]["data"][0]["input"], "{\"path\":\"missing.rs\"}");
    assert_eq!(messages[2]["type"], "tool_result");
    assert_eq!(messages[2]["data"]["tool_call_id"], "missing-file");
    assert_eq!(messages[2]["data"]["is_error"], true);
    assert_eq!(messages[3]["data"], "file not found");
}

#[tokio::test]
async fn rejects_existing_file_before_model_request() {
    let server = MockServer::start().await;
    let config = config(&server);
    let existing = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(existing.path(), "do not overwrite").unwrap();
    Command::cargo_bin("agent-cli-rust")
        .unwrap()
        .arg("claw")
        .arg(config.path())
        .arg("--interactive")
        .arg("--session-file")
        .arg(existing.path())
        .write_stdin("hello\n")
        .timeout(Duration::from_secs(5))
        .assert()
        .failure()
        .stderr(contains("session file already exists"));
    assert!(server.received_requests().await.unwrap().is_empty());
    assert_eq!(
        std::fs::read_to_string(existing.path()).unwrap(),
        "do not overwrite"
    );
}

#[tokio::test]
async fn reports_save_failure_after_printing_completed_answer() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("history.json");
    let collision = path.clone();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(move |_: &wiremock::Request| {
            std::fs::write(&collision, "external file").unwrap();
            ResponseTemplate::new(200)
                .set_body_json(json!({"choices":[{"message":{"content":"answer completed"}}]}))
        })
        .expect(1)
        .mount(&server)
        .await;
    let config = config(&server);
    Command::cargo_bin("agent-cli-rust")
        .unwrap()
        .arg("claw")
        .arg(config.path())
        .arg("--interactive")
        .arg("--session-file")
        .arg(&path)
        .write_stdin("hello\nnext\n")
        .timeout(Duration::from_secs(10))
        .assert()
        .failure()
        .stdout(contains("answer completed"))
        .stderr(contains("answer completed, but session save failed"));
    assert_eq!(std::fs::read_to_string(path).unwrap(), "external file");
}

#[tokio::test]
async fn new_conversation_keeps_old_archive_and_can_resume_it() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(|request: &wiremock::Request| {
            let body: Value = request.body_json().unwrap();
            let messages = body["messages"].as_array().unwrap();
            let prompt = messages.last().unwrap()["content"].as_str().unwrap();
            match prompt {
                "first\n" | "second\n" => assert_eq!(messages.len(), 1),
                "continue\n" => {
                    assert_eq!(messages.len(), 3);
                    assert_eq!(messages[0]["content"], "first\n");
                    assert_eq!(messages[1]["content"], "answer:first");
                }
                _ => panic!("unexpected model input: {messages:?}"),
            }
            ResponseTemplate::new(200).set_body_json(json!({"choices":[{"message":{
                "content":format!("answer:{}", prompt.trim())
            }}]}))
        })
        .expect(3)
        .mount(&server)
        .await;
    let config = config(&server);
    let directory = tempfile::tempdir().unwrap();
    Command::cargo_bin("agent-cli-rust")
        .unwrap()
        .arg("claw")
        .arg(config.path())
        .arg("--interactive")
        .arg("--workspace")
        .arg(directory.path())
        .args(["--max-context-bytes", "256"])
        .write_stdin("first\n/new\nsecond\n/new\n/exit\n")
        .timeout(Duration::from_secs(10))
        .assert()
        .success()
        .stderr(contains("已开启新对话"));
    let files: Vec<_> = std::fs::read_dir(directory.path().join(".agent-cli/sessions"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(
        files.len(),
        2,
        "empty conversations must not create archives"
    );
    let mut old = None;
    let mut second = None;
    for path in files {
        let bytes = std::fs::read(&path).unwrap();
        let saved: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(saved["full_history"].as_array().unwrap().len(), 2);
        match saved["full_history"][0]["data"].as_str().unwrap() {
            "first\n" => old = Some(path),
            "second\n" => second = Some((path, bytes)),
            other => panic!("unexpected archived input: {other}"),
        }
    }
    let old = old.unwrap();
    let (second, second_bytes) = second.unwrap();
    Command::cargo_bin("agent-cli-rust")
        .unwrap()
        .arg("claw")
        .arg(config.path())
        .arg("--interactive")
        .arg("--workspace")
        .arg(directory.path())
        .arg("--resume")
        .arg(&old)
        .write_stdin("continue\n/exit\n")
        .timeout(Duration::from_secs(10))
        .assert()
        .success();
    let saved: Value = serde_json::from_slice(&std::fs::read(old).unwrap()).unwrap();
    assert_eq!(saved["full_history"].as_array().unwrap().len(), 4);
    assert_eq!(std::fs::read(second).unwrap(), second_bytes);
}

#[tokio::test]
async fn failed_new_allocation_retains_history_and_explicit_archive() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(|request: &wiremock::Request| {
            let body: Value = request.body_json().unwrap();
            let messages = body["messages"].as_array().unwrap();
            if messages.last().unwrap()["content"] == "next\n" {
                assert_eq!(messages.len(), 3);
                assert_eq!(messages[0]["content"], "hello\n");
            } else {
                assert_eq!(messages.len(), 1);
            }
            ResponseTemplate::new(200)
                .set_body_json(json!({"choices":[{"message":{"content":"ok"}}]}))
        })
        .expect(2)
        .mount(&server)
        .await;
    let config = config(&server);
    let directory = tempfile::tempdir().unwrap();
    // Explicit archives still work, but automatic allocation must reject this path.
    std::fs::write(directory.path().join(".agent-cli"), "user file").unwrap();
    let archive = directory.path().join("old.json");
    Command::cargo_bin("agent-cli-rust")
        .unwrap()
        .arg("claw")
        .arg(config.path())
        .arg("--interactive")
        .arg("--workspace")
        .arg(directory.path())
        .arg("--session-file")
        .arg(&archive)
        .write_stdin("hello\n/new\nnext\n/exit\n")
        .timeout(Duration::from_secs(10))
        .assert()
        .success()
        .stderr(contains("无法开启新对话，当前会话已保留"));
    let saved: Value = serde_json::from_slice(&std::fs::read(archive).unwrap()).unwrap();
    assert_eq!(saved["full_history"].as_array().unwrap().len(), 4);
    assert_eq!(
        std::fs::read_to_string(directory.path().join(".agent-cli")).unwrap(),
        "user file"
    );
}

#[tokio::test]
async fn sessions_lists_archives_without_model_calls_or_context_changes() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(|request: &wiremock::Request| {
            let body: Value = request.body_json().unwrap();
            let messages = body["messages"].as_array().unwrap();
            match messages.len() {
                1 => assert_eq!(messages[0]["content"], "hello\n"),
                3 => {
                    assert_eq!(messages[0]["content"], "hello\n");
                    assert_eq!(messages[1]["content"], "ok");
                    assert_eq!(messages[2]["content"], "next\n");
                }
                _ => panic!("listing changed context: {messages:?}"),
            }
            ResponseTemplate::new(200)
                .set_body_json(json!({"choices":[{"message":{"content":"ok"}}]}))
        })
        .expect(2)
        .mount(&server)
        .await;
    let config = config(&server);
    let directory = tempfile::tempdir().unwrap();
    Command::cargo_bin("agent-cli-rust")
        .unwrap()
        .arg("claw")
        .arg(config.path())
        .arg("--interactive")
        .arg("--workspace")
        .arg(directory.path())
        .write_stdin("/sessions\nhello\n/sessions\nnext\n/exit\n")
        .timeout(Duration::from_secs(10))
        .assert()
        .success()
        .stderr(contains("暂无可显示的存档。"))
        .stderr(contains("hello\\n"));
    let archives: Vec<_> = std::fs::read_dir(directory.path().join(".agent-cli/sessions"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(archives.len(), 1);
    let before = std::fs::read(&archives[0]).unwrap();
    Command::cargo_bin("agent-cli-rust")
        .unwrap()
        .arg("claw")
        .arg(config.path())
        .arg("--interactive")
        .arg("--workspace")
        .arg(directory.path())
        .write_stdin("/sessions\n/exit\n")
        .timeout(Duration::from_secs(10))
        .assert()
        .success()
        .stderr(contains("hello\\n"));
    assert_eq!(std::fs::read(&archives[0]).unwrap(), before);
    assert_eq!(
        std::fs::read_dir(directory.path().join(".agent-cli/sessions"))
            .unwrap()
            .count(),
        1
    );
}

fn automatic_fixture(root: &std::path::Path, name: &str) -> std::path::PathBuf {
    let archives = root.join(".agent-cli/sessions");
    std::fs::create_dir_all(&archives).unwrap();
    let path = archives.join(name);
    std::fs::write(&path, serde_json::to_vec(&json!({
        "version":1,"workspace":root.canonicalize().unwrap(),
        "messages":[{"type":"user","data":"old question"},{"type":"assistant","data":"old answer"}]
    })).unwrap()).unwrap();
    path
}

#[tokio::test]
async fn interactive_resume_uses_displayed_number_and_preserves_budget_permissions_and_archives() {
    let directory = tempfile::tempdir().unwrap();
    let old = automatic_fixture(directory.path(), "1-old.json");
    let root = directory.path().to_path_buf();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(move |request: &wiremock::Request| {
            let body: Value = request.body_json().unwrap();
            let messages = body["messages"].as_array().unwrap();
            match messages.last().unwrap()["content"].as_str().unwrap() {
                "current\n" => {
                    assert_eq!(messages.len(), 1);
                    // Appears AFTER /sessions: selecting 1 must not re-sort the directory.
                    automatic_fixture(&root, "999999999999999999-newer.json");
                }
                "continue\n" => {
                    assert_eq!(messages.len(), 3);
                    assert_eq!(messages[0]["content"], "old question");
                    assert_eq!(messages[1]["content"], "old answer");
                }
                other => panic!("unexpected request (budget or command leaked): {other}"),
            }
            let tools = body["tools"].as_array().unwrap();
            assert!(
                tools
                    .iter()
                    .any(|tool| tool["function"]["name"] == "create_file")
            );
            assert!(
                !tools
                    .iter()
                    .any(|tool| tool["function"]["name"] == "run_command")
            );
            ResponseTemplate::new(200)
                .set_body_json(json!({"choices":[{"message":{"content":"ok"}}]}))
        })
        .expect(2)
        .mount(&server)
        .await;
    let config = config(&server);
    Command::cargo_bin("agent-cli-rust")
        .unwrap()
        .arg("claw")
        .arg(config.path())
        .arg("--interactive")
        .arg("--workspace")
        .arg(directory.path())
        .args(["--allow-write", "--max-context-bytes", "256"])
        .write_stdin(format!(
            "/sessions\ncurrent\n/new\n/resume 1\n{}\ncontinue\n/exit\n",
            "x".repeat(512)
        ))
        .timeout(Duration::from_secs(10))
        .assert()
        .success()
        .stderr(contains("已恢复所选对话"))
        .stderr(contains("本次输入未发送"));
    let saved: Value = serde_json::from_slice(&std::fs::read(&old).unwrap()).unwrap();
    assert_eq!(saved["full_history"].as_array().unwrap().len(), 4);
    assert_eq!(saved["full_history"][2]["data"], "continue\n");
    let mut found_current = false;
    for entry in std::fs::read_dir(directory.path().join(".agent-cli/sessions")).unwrap() {
        let path = entry.unwrap().path();
        if path == old || path.file_name().unwrap() == "999999999999999999-newer.json" {
            continue;
        }
        let saved: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(saved["full_history"].as_array().unwrap().len(), 2);
        assert_eq!(saved["full_history"][0]["data"], "current\n");
        found_current = true;
    }
    assert!(found_current);
}

#[tokio::test]
async fn interactive_resume_invalid_arguments_and_missing_archive_preserve_current_history() {
    let directory = tempfile::tempdir().unwrap();
    let old = automatic_fixture(directory.path(), "1-old.json");
    let removed = old.clone();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(move |request: &wiremock::Request| {
            let body: Value = request.body_json().unwrap();
            let messages = body["messages"].as_array().unwrap();
            if messages.len() == 1 {
                assert_eq!(messages[0]["content"], "current\n");
                std::fs::remove_file(&removed).unwrap();
            } else {
                assert_eq!(messages.len(), 3);
                assert_eq!(messages[0]["content"], "current\n");
                assert_eq!(messages[2]["content"], "next\n");
            }
            ResponseTemplate::new(200)
                .set_body_json(json!({"choices":[{"message":{"content":"ok"}}]}))
        })
        .expect(2)
        .mount(&server)
        .await;
    let config = config(&server);
    Command::cargo_bin("agent-cli-rust").unwrap()
        .arg("claw").arg(config.path()).arg("--interactive")
        .arg("--workspace").arg(directory.path())
        .write_stdin("/resume 1\n/sessions\n/resume\n/resume 0\n/resume 99\n/resume abc\n/resume 1 extra\ncurrent\n/resume 1\nnext\n/exit\n")
        .timeout(Duration::from_secs(10)).assert().success()
        .stderr(contains("用法：/resume <有效编号>"))
        .stderr(contains("无法恢复会话，当前对话已保留"));
    assert!(!old.exists());
    let archives: Vec<_> = std::fs::read_dir(directory.path().join(".agent-cli/sessions"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(archives.len(), 1);
    let saved: Value = serde_json::from_slice(&std::fs::read(&archives[0]).unwrap()).unwrap();
    assert_eq!(saved["full_history"].as_array().unwrap().len(), 4);
    assert_eq!(saved["full_history"][0]["data"], "current\n");
}
