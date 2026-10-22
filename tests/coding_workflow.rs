//! Deterministic coding workflow: scripted model decisions, real CLI and tools.
//! This verifies orchestration, not a live model's ability to diagnose a bug.
use std::{io::Write, time::Duration};

use assert_cmd::Command;
use predicates::str::contains;
use serde_json::{Value, json};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

fn tool_response(id: &str, name: &str, arguments: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "choices": [{"message": {"content": null, "tool_calls": [{
            "id": id, "type": "function",
            "function": {"name": name, "arguments": arguments.to_string()}
        }]}}]
    }))
}

// Failed command results are encoded as an error string containing command JSON.
fn command_result(message: &Value) -> Value {
    let content: Value = serde_json::from_str(message["content"].as_str().unwrap()).unwrap();
    match content.get("error") {
        Some(error) => serde_json::from_str(error.as_str().unwrap()).unwrap(),
        None => content,
    }
}

#[tokio::test]
async fn claw_repairs_failing_test_without_changing_assertions() {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::create_dir(workspace.path().join("src")).unwrap();
    std::fs::create_dir(workspace.path().join("tests")).unwrap();
    let manifest = "[package]\nname = \"repair_fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\n";
    let broken = "pub fn add(a: i32, b: i32) -> i32 {\n    a - b\n}\n";
    let tests =
        "#[test]\nfn adds_two_numbers() {\n    assert_eq!(repair_fixture::add(2, 3), 5);\n}\n";
    std::fs::write(workspace.path().join("Cargo.toml"), manifest).unwrap();
    std::fs::write(workspace.path().join("src/lib.rs"), broken).unwrap();
    std::fs::write(workspace.path().join("tests/add.rs"), tests).unwrap();

    let server = MockServer::start().await;
    let cargo_args = json!({
        "program": env!("CARGO"),
        "args": ["test", "--offline", "--manifest-path", "Cargo.toml", "--target-dir", "target"],
        "timeout_seconds": 60
    });
    let request_args = cargo_args.clone();
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(move |request: &wiremock::Request| {
            let body: Value = request.body_json().unwrap();
            match body["messages"].as_array().unwrap().len() {
                1 => tool_response("test-before", "run_command", request_args.clone()),
                3 => tool_response("read-source", "read_file", json!({"path": "src/lib.rs"})),
                5 => tool_response(
                    "fix-source",
                    "replace_text",
                    json!({
                        "path": "src/lib.rs", "old_text": "a - b", "new_text": "a + b"
                    }),
                ),
                7 => tool_response("test-after", "run_command", request_args.clone()),
                9 => ResponseTemplate::new(200).set_body_json(json!({
                    "choices": [{"message": {"content": "Implementation repaired; tests pass."}}]
                })),
                _ => ResponseTemplate::new(500),
            }
        })
        .expect(5)
        .mount(&server)
        .await;

    let mut config = tempfile::NamedTempFile::new().unwrap();
    write!(
        config,
        "[llm]\nbase_url = {:?}\napi_key = \"test-key\"\nmodel = \"scripted-model\"\n",
        server.uri()
    )
    .unwrap();
    Command::cargo_bin("agent-cli-rust").unwrap()
        .arg("claw").arg(config.path())
        .arg("Run tests, repair the implementation without modifying assertions, then rerun tests.")
        .arg("--workspace").arg(workspace.path())
        .args(["--allow-write", "--allow-exec", "--max-iterations", "5", "--max-tool-calls", "4"])
        .timeout(Duration::from_secs(150))
        .assert().success()
        .stdout(contains("Implementation repaired; tests pass.\n\n执行统计：\n工具调用：\n  read_file: 1 次\n  replace_text: 1 次\n  run_command: 2 次\n工具调用总数：4\nLoop 次数：5\n"));

    // Inspect actual request history, not the scripted model's success statement.
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 5);
    let final_request: Value = requests[4].body_json().unwrap();
    let messages = final_request["messages"].as_array().unwrap();
    let expected_calls = [
        ("test-before", "run_command"),
        ("read-source", "read_file"),
        ("fix-source", "replace_text"),
        ("test-after", "run_command"),
    ];
    for (index, (id, name)) in expected_calls.iter().enumerate() {
        let assistant = &messages[index * 2 + 1];
        let result = &messages[index * 2 + 2];
        assert_eq!(assistant["role"], "assistant");
        assert_eq!(assistant["tool_calls"].as_array().unwrap().len(), 1);
        assert_eq!(assistant["tool_calls"][0]["id"], *id);
        assert_eq!(assistant["tool_calls"][0]["function"]["name"], *name);
        assert_eq!(result["role"], "tool");
        assert_eq!(result["tool_call_id"], *id);
    }
    // Each subsequent model request must preserve the complete prior prefix.
    for (index, request) in requests.iter().enumerate() {
        let body: Value = request.body_json().unwrap();
        assert_eq!(body["messages"], json!(&messages[..index * 2 + 1]));
    }
    let failed = command_result(&messages[2]);
    assert_eq!(failed["success"], false);
    assert_eq!(failed["exit_code"], 101);
    assert_eq!(failed["timed_out"], false);
    assert!(
        failed["stdout"]
            .as_str()
            .unwrap()
            .contains("adds_two_numbers ... FAILED")
    );
    let read: Value = serde_json::from_str(messages[4]["content"].as_str().unwrap()).unwrap();
    assert_eq!(read["content"], broken);
    let edit: Value = serde_json::from_str(messages[6]["content"].as_str().unwrap()).unwrap();
    assert_eq!(edit["applied"], true);
    assert!(edit["diff"].as_str().unwrap().contains("+    a + b"));
    let passed = command_result(&messages[8]);
    assert_eq!(passed["success"], true);
    assert_eq!(passed["exit_code"], 0);
    assert_eq!(passed["timed_out"], false);
    assert!(
        passed["stdout"]
            .as_str()
            .unwrap()
            .contains("adds_two_numbers ... ok")
    );

    assert_eq!(
        std::fs::read_to_string(workspace.path().join("src/lib.rs")).unwrap(),
        broken.replace("a - b", "a + b")
    );
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("tests/add.rs")).unwrap(),
        tests
    );
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("Cargo.toml")).unwrap(),
        manifest
    );

    // Independently verify the resulting project, outside the Agent's tool path.
    let output = tokio::time::timeout(
        Duration::from_secs(60),
        tokio::process::Command::new(env!("CARGO"))
            .args([
                "test",
                "--offline",
                "--manifest-path",
                "Cargo.toml",
                "--target-dir",
                "target",
            ])
            .current_dir(workspace.path())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .expect("independent verification timed out")
    .unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("adds_two_numbers ... ok"));
}
