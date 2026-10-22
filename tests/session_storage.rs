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
    assert_eq!(snapshot["messages"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn saves_each_completed_turn_only_when_explicitly_requested() {
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
        if save {
            let text = std::fs::read_to_string(&path).unwrap();
            let snapshot: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(snapshot["version"], 1);
            assert_eq!(
                snapshot["workspace"],
                directory.path().canonicalize().unwrap().to_str().unwrap()
            );
            assert_eq!(
                snapshot["messages"],
                json!([
                    {"type":"user","data":"options\n"},
                    {"type":"assistant","data":"1. choose this"},
                    {"type":"user","data":"1\n"},
                    {"type":"assistant","data":"confirmed"}
                ])
            );
            assert!(!text.contains("private-test-key"));
        } else {
            assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
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
    assert_eq!(snapshot["messages"].as_array().unwrap().len(), 2);
    assert_eq!(snapshot["messages"][1]["data"], "saved answer");
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
    let messages = snapshot["messages"].as_array().unwrap();
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
