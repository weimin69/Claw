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
            "tools": [add_tool.clone()]
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
            "tools": [add_tool]
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
