use assert_cmd::Command;
use predicates::str::contains;
use serde_json::{Value, json};
use std::{io::Write, path::Path, time::Duration};
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};

fn config(server: &MockServer) -> tempfile::NamedTempFile {
    let mut config = tempfile::NamedTempFile::new().unwrap();
    write!(
        config,
        "[llm]\nbase_url = {:?}\napi_key = \"test-key\"\nmodel = \"mock\"\n",
        server.uri()
    )
    .unwrap();
    config
}

fn snapshot(path: &Path, workspace: &Path) -> Vec<u8> {
    let bytes = serde_json::to_vec_pretty(&json!({
        "version":1, "workspace": workspace.canonicalize().unwrap(),
        "messages":[{"type":"user","data":"options?"}, {"type":"assistant","data":"1. clear; 2. keep"}]
    })).unwrap();
    std::fs::write(path, &bytes).unwrap();
    bytes
}

#[tokio::test]
async fn trims_old_turn_then_restores_trimmed_context_and_full_history() {
    let server = MockServer::start().await;
    let config = config(&server);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("v2.json");
    let working = json!([
        {"type":"user","data":"recent request"},
        {"type":"assistant","data":"recent answer"}
    ]);
    let full = json!([
        {"type":"user","data":"original details ".repeat(256)},
        {"type":"assistant","data":"old answer"},
        working[0], working[1]
    ]);
    std::fs::write(
        &path,
        serde_json::to_vec(&json!({
            "version":2,"workspace":directory.path().canonicalize().unwrap(),
            "full_history":full,"working_context":full
        }))
        .unwrap(),
    )
    .unwrap();
    Mock::given(method("POST"))
        .respond_with(|request: &wiremock::Request| {
            let body: Value = request.body_json().unwrap();
            let mut expected = json!([
                {"role":"user","content":"recent request"},
                {"role":"assistant","content":"recent answer"},
                {"role":"user","content":"next\n"}
            ]);
            if body["messages"].as_array().unwrap().len() == 5 {
                expected.as_array_mut().unwrap().extend([
                    json!({"role":"assistant","content":"continued"}),
                    json!({"role":"user","content":"again\n"}),
                ]);
            }
            assert_eq!(body["messages"], expected);
            ResponseTemplate::new(200)
                .set_body_json(json!({"choices":[{"message":{"content":"continued"}}]}))
        })
        .expect(2)
        .mount(&server)
        .await;
    Command::cargo_bin("agent-cli-rust")
        .unwrap()
        .arg("claw")
        .arg(config.path())
        .arg("--interactive")
        .arg("--workspace")
        .arg(directory.path())
        .arg("--resume")
        .arg(&path)
        .args(["--max-context-bytes", "512"])
        .write_stdin("next\n/exit\n")
        .timeout(Duration::from_secs(10))
        .assert()
        .success()
        .stderr(contains("上下文已移除最旧的 1 轮对话"));
    let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved["version"], 3);
    assert_eq!(
        &saved["full_history"].as_array().unwrap()[..4],
        full.as_array().unwrap()
    );
    assert_eq!(saved["full_history"].as_array().unwrap().len(), 6);
    assert_eq!(saved["context"], json!({"prefix":[],"recent_start":2}));
    assert!(saved.get("working_context").is_none());
    assert_eq!(saved["full_history"][5]["data"], "continued");

    Command::cargo_bin("agent-cli-rust")
        .unwrap()
        .arg("claw")
        .arg(config.path())
        .arg("--interactive")
        .arg("--workspace")
        .arg(directory.path())
        .arg("--resume")
        .arg(&path)
        .args(["--max-context-bytes", "512"])
        .write_stdin("again\n/exit\n")
        .timeout(Duration::from_secs(10))
        .assert()
        .success();
    let restored: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(restored["context"], json!({"prefix":[],"recent_start":2}));
    assert_eq!(restored["full_history"].as_array().unwrap().len(), 8);
    assert_eq!(
        &restored["full_history"].as_array().unwrap()[..6],
        saved["full_history"].as_array().unwrap()
    );
}

#[tokio::test]
async fn resumes_across_processes_updates_file_and_does_not_restore_permissions() {
    let server = MockServer::start().await;
    let config = config(&server);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("history.json");
    Mock::given(method("POST")).respond_with(|request: &wiremock::Request| {
        let body: Value = request.body_json().unwrap();
        let messages = body["messages"].as_array().unwrap();
        let message = match messages.len() {
            1 => {
                assert_eq!(messages[0]["content"], "options?\n");
                json!({"content":"1. clear; 2. keep"})
            }
            3 => {
                assert_eq!(messages[0]["content"], "options?\n");
                assert_eq!(messages[1]["role"], "assistant");
                assert_eq!(messages[1]["content"], "1. clear; 2. keep");
                assert_eq!(messages[2]["content"], "1");
                let tools = body["tools"].as_array().unwrap();
                assert!(!tools.iter().any(|t| matches!(t["function"]["name"].as_str(), Some("create_file" | "replace_text" | "run_command"))));
                // Forge a hidden call: executor permission checks must still hold.
                json!({"content":null,"tool_calls":[{"id":"new-call","type":"function",
                    "function":{"name":"create_file","arguments":"{\"path\":\"marker\",\"content\":\"forbidden\"}"}}]})
            }
            5 => {
                assert_eq!(messages[4]["tool_call_id"], "new-call");
                assert!(messages[4]["content"].as_str().unwrap().contains("write access denied"));
                json!({"content":"I remember option 1; current permissions prohibit writing."})
            }
            _ => panic!("unexpected history length"),
        };
        ResponseTemplate::new(200).set_body_json(json!({"choices":[{"message":message}]}))
    }).expect(3).mount(&server).await;
    Command::cargo_bin("agent-cli-rust")
        .unwrap()
        .arg("claw")
        .arg(config.path())
        .arg("--interactive")
        .arg("--workspace")
        .arg(directory.path())
        .args(["--allow-write", "--allow-exec"])
        .arg("--session-file")
        .arg(&path)
        .write_stdin("options?\n/exit\n")
        .timeout(Duration::from_secs(10))
        .assert()
        .success();
    Command::cargo_bin("agent-cli-rust")
        .unwrap()
        .arg("claw")
        .arg(config.path())
        .arg("1")
        .arg("--interactive")
        .arg("--workspace")
        .arg(directory.path())
        .arg("--resume")
        .arg(&path)
        .args(["--max-iterations", "2", "--max-tool-calls", "1"])
        .write_stdin("/exit\n")
        .timeout(Duration::from_secs(10))
        .assert()
        .success()
        .stdout(contains("I remember option 1"))
        .stderr(contains("已恢复会话"));
    assert!(!directory.path().join("marker").exists());
    let saved: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(saved["full_history"].as_array().unwrap().len(), 6);
    assert_eq!(saved["full_history"][4]["data"]["is_error"], true);
    assert!(
        saved["full_history"][5]["data"]
            .as_str()
            .unwrap()
            .starts_with("I remember")
    );
}

#[tokio::test]
async fn failed_resumed_request_does_not_modify_snapshot() {
    let server = MockServer::start().await;
    let config = config(&server);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("history.json");
    let before = snapshot(&path, directory.path());
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .expect(1)
        .mount(&server)
        .await;
    Command::cargo_bin("agent-cli-rust")
        .unwrap()
        .arg("claw")
        .arg(config.path())
        .arg("--interactive")
        .arg("--workspace")
        .arg(directory.path())
        .arg("--resume")
        .arg(&path)
        .write_stdin("1\n")
        .timeout(Duration::from_secs(10))
        .assert()
        .failure();
    assert_eq!(std::fs::read(path).unwrap(), before);
}

#[tokio::test]
async fn restored_tool_history_is_sent_as_context_not_replayed() {
    let server = MockServer::start().await;
    let config = config(&server);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("history.json");
    let arguments = "{\"path\":\"marker\",\"content\":\"old operation\"}";
    std::fs::write(&path, serde_json::to_vec(&json!({
        "version":1,"workspace":directory.path().canonicalize().unwrap(),
        "messages":[
            {"type":"user","data":"create marker"},
            {"type":"assistant_tool_calls","data":[{"id":"old","name":"create_file","input":arguments}]},
            {"type":"tool_result","data":{"tool_call_id":"old","name":"create_file","output":"created","is_error":false}},
            {"type":"assistant","data":"previous answer"}
        ]
    })).unwrap()).unwrap();
    Mock::given(method("POST"))
        .respond_with(move |request: &wiremock::Request| {
            let body: Value = request.body_json().unwrap();
            assert_eq!(body["messages"].as_array().unwrap().len(), 5);
            assert_eq!(body["messages"][1]["tool_calls"][0]["id"], "old");
            assert_eq!(
                body["messages"][1]["tool_calls"][0]["function"]["arguments"],
                arguments
            );
            assert_eq!(body["messages"][2]["tool_call_id"], "old");
            assert_eq!(body["messages"][3]["content"], "previous answer");
            assert_eq!(body["messages"][4]["content"], "continue\n");
            ResponseTemplate::new(200)
                .set_body_json(json!({"choices":[{"message":{"content":"continued"}}]}))
        })
        .expect(1)
        .mount(&server)
        .await;
    // Write access is enabled, so absence of marker proves calls weren't replayed.
    Command::cargo_bin("agent-cli-rust")
        .unwrap()
        .arg("claw")
        .arg(config.path())
        .arg("--interactive")
        .arg("--allow-write")
        .arg("--workspace")
        .arg(directory.path())
        .arg("--resume")
        .arg(&path)
        .write_stdin("continue\n/exit\n")
        .timeout(Duration::from_secs(10))
        .assert()
        .success()
        .stdout(contains("工具调用总数：0\nLoop 次数：1"));
    assert!(!directory.path().join("marker").exists());
}

#[tokio::test]
async fn immediate_exit_does_not_rewrite_resumed_file() {
    let server = MockServer::start().await;
    let config = config(&server);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("history.json");
    let original = snapshot(&path, directory.path());
    Command::cargo_bin("agent-cli-rust")
        .unwrap()
        .arg("claw")
        .arg(config.path())
        .arg("--interactive")
        .arg("--workspace")
        .arg(directory.path())
        .arg("--resume")
        .arg(&path)
        .write_stdin("/exit\n")
        .timeout(Duration::from_secs(5))
        .assert()
        .success();
    assert_eq!(std::fs::read(path).unwrap(), original);
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn rejects_invalid_resume_before_sending_model_requests() {
    let server = MockServer::start().await;
    let config = config(&server);
    let directory = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let path = directory.path().join("history.json");
    for (messages, version, root, expected) in [
        (json!([]), 1, directory.path(), "history is empty"),
        (
            json!([{"type":"user","data":"unfinished"}]),
            1,
            directory.path(),
            "incomplete turn",
        ),
        (
            json!([]),
            99,
            directory.path(),
            "unsupported session version",
        ),
        (json!([]), 1, other.path(), "workspace does not match"),
    ] {
        std::fs::write(
            &path,
            serde_json::to_vec(&json!({"version":version,"workspace":root,"messages":messages}))
                .unwrap(),
        )
        .unwrap();
        Command::cargo_bin("agent-cli-rust")
            .unwrap()
            .arg("claw")
            .arg(config.path())
            .arg("--interactive")
            .arg("--workspace")
            .arg(directory.path())
            .arg("--resume")
            .arg(&path)
            .write_stdin("1\n")
            .timeout(Duration::from_secs(5))
            .assert()
            .failure()
            .stderr(contains(expected));
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[test]
fn resume_requires_interactive_and_conflicts_with_new_session_file() {
    for args in [
        vec!["--resume", "old.json"],
        vec![
            "--interactive",
            "--resume",
            "old.json",
            "--session-file",
            "new.json",
        ],
    ] {
        Command::cargo_bin("agent-cli-rust")
            .unwrap()
            .args(["claw", "unused.toml"])
            .args(args)
            .assert()
            .code(2);
    }
}

#[tokio::test]
async fn v2_restores_distinct_context_and_preserves_full_history_on_update() {
    let server = MockServer::start().await;
    let config = config(&server);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("v2.json");
    let working = json!([
        {"type":"user","data":"recent request"},
        {"type":"assistant","data":"recent answer"}
    ]);
    let full = json!([
        {"type":"user","data":"original details ".repeat(256)},
        {"type":"assistant","data":"old answer"},
        working[0], working[1]
    ]);
    std::fs::write(
        &path,
        serde_json::to_vec(&json!({
            "version":2,"workspace":directory.path().canonicalize().unwrap(),
            "full_history":full,"working_context":working
        }))
        .unwrap(),
    )
    .unwrap();
    Mock::given(method("POST"))
        .respond_with(|request: &wiremock::Request| {
            let body: Value = request.body_json().unwrap();
            assert_eq!(
                body["messages"],
                json!([
                    {"role":"user","content":"recent request"},
                    {"role":"assistant","content":"recent answer"},
                    {"role":"user","content":"next\n"}
                ])
            );
            ResponseTemplate::new(200)
                .set_body_json(json!({"choices":[{"message":{"content":"continued"}}]}))
        })
        .expect(1)
        .mount(&server)
        .await;
    Command::cargo_bin("agent-cli-rust")
        .unwrap()
        .arg("claw")
        .arg(config.path())
        .arg("--interactive")
        .arg("--workspace")
        .arg(directory.path())
        .arg("--resume")
        .arg(&path)
        .args(["--max-context-bytes", "512"])
        .write_stdin("next\n/exit\n")
        .timeout(Duration::from_secs(10))
        .assert()
        .success();
    let saved: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(saved["version"], 3);
    assert_eq!(
        &saved["full_history"].as_array().unwrap()[..4],
        full.as_array().unwrap()
    );
    assert_eq!(saved["full_history"].as_array().unwrap().len(), 6);
    assert_eq!(saved["context"], json!({"prefix":[],"recent_start":2}));
    assert!(saved.get("working_context").is_none());
    assert_eq!(saved["full_history"][5]["data"], "continued");
}
