use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::{Value, json};
use std::{io::Write, time::Duration};
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};

fn config(server: &MockServer) -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    write!(
        file,
        "[llm]\nbase_url = {:?}\napi_key = \"test-key\"\nmodel = \"mock\"\n",
        server.uri()
    )
    .unwrap();
    file
}

#[tokio::test]
async fn interactive_rejects_large_input_then_saves_only_valid_turn() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(|request: &wiremock::Request| {
            let body: Value = request.body_json().unwrap();
            assert_eq!(
                body["messages"],
                json!([{"role":"user","content":"hello\n"}])
            );
            ResponseTemplate::new(200)
                .set_body_json(json!({"choices":[{"message":{"content":"hi"}}]}))
        })
        .expect(1)
        .mount(&server)
        .await;
    let config = config(&server);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.json");
    Command::cargo_bin("agent-cli-rust")
        .unwrap()
        .arg("claw")
        .arg(config.path())
        .arg("--interactive")
        .args(["--max-context-bytes", "512"])
        .arg("--session-file")
        .arg(&path)
        .write_stdin(format!("{}\nhello\n/exit\n", "x".repeat(2048)))
        .timeout(Duration::from_secs(10))
        .assert()
        .success()
        .stdout(predicate::str::contains("hi"))
        .stderr(predicate::str::contains("本次输入未发送"));
    let saved: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(saved["full_history"].as_array().unwrap().len(), 2);
    assert_eq!(saved["full_history"][0]["data"], "hello\n");
}

#[tokio::test]
async fn noninteractive_rejects_before_http_and_cli_rejects_zero_budget() {
    let server = MockServer::start().await;
    let config = config(&server);
    Command::cargo_bin("agent-cli-rust")
        .unwrap()
        .arg("claw")
        .arg(config.path())
        .arg("large input")
        .args(["--max-context-bytes", "1"])
        .timeout(Duration::from_secs(5))
        .assert()
        .failure()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("context exceeds size limit"));
    assert!(server.received_requests().await.unwrap().is_empty());
    Command::cargo_bin("agent-cli-rust")
        .unwrap()
        .args(["claw", "unused.toml", "--max-context-bytes", "0"])
        .assert()
        .code(2);
}

#[tokio::test]
async fn tool_growth_preserves_resumed_snapshot_and_stops_before_next_request() {
    let server = MockServer::start().await;
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(200).set_body_json(json!({
        "choices":[{"message":{"content":null,"tool_calls":[{
            "id":"read","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"large.txt\"}"}
        }]}}]
    }))).expect(1).mount(&server).await;
    let config = config(&server);
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("large.txt"), "x".repeat(8192)).unwrap();
    let path = directory.path().join("session.json");
    let before = serde_json::to_vec(
        &json!({"version":1,"workspace":directory.path().canonicalize().unwrap(),"messages":[
            {"type":"user","data":"previous input"},{"type":"assistant","data":"previous answer"}
        ]}),
    )
    .unwrap();
    std::fs::write(&path, &before).unwrap();
    Command::cargo_bin("agent-cli-rust")
        .unwrap()
        .arg("claw")
        .arg(config.path())
        .arg("--interactive")
        .arg("--workspace")
        .arg(directory.path())
        .arg("--resume")
        .arg(&path)
        .args(["--max-context-bytes", "1024"])
        .write_stdin("read the file\n")
        .timeout(Duration::from_secs(10))
        .assert()
        .failure()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("工具执行后上下文超限"));
    assert_eq!(std::fs::read(path).unwrap(), before);
}
