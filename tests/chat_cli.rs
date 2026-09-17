use std::io::Write;

use assert_cmd::Command;
use predicates::str::{contains, is_empty};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_json, header, method, path},
};

fn write_temp_config(contents: &str) -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(contents.as_bytes()).unwrap();
    file
}

#[tokio::test]
async fn chat_prints_assistant_response_from_mock_server() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("authorization", "Bearer test-key"))
        .and(body_json(serde_json::json!({
            "model": "test-model",
            "temperature": 0.7,
            "messages": [
                {
                    "role": "user",
                    "content": "hello"
                }
            ],
            "stream": false
        })))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{
                "choices": [
                    {
                        "message": {
                            "content": "Hello from mock assistant"
                        }
                    }
                ]
            }"#,
        ))
        .mount(&server)
        .await;

    let config_file = write_temp_config(&format!(
        r#"[llm]
    base_url = "{}"
    api_key = "test-key"
    model = "test-model"
    temperature = 0.7
    "#,
        server.uri()
    ));

    let config_path = config_file.path().to_str().unwrap();

    let mut cmd = Command::cargo_bin("agent-cli-rust").unwrap();

    cmd.arg("chat")
        .arg(config_path)
        .arg("hello")
        .assert()
        .success()
        .stdout(contains("Hello from mock assistant"))
        .stderr(is_empty());
}

#[test]
fn chat_rejects_missing_api_key() {
    let config_file = write_temp_config(
        r#"[llm]
    base_url = "https://127.0.0.1:1"
    model = "test-model"
    temperature = 0.7
    "#,
    );
    let config_path = config_file.path().to_str().unwrap();

    let mut cmd = Command::cargo_bin("agent-cli-rust").unwrap();

    cmd.arg("chat")
        .arg(config_path)
        .arg("hello")
        .assert()
        .failure()
        .stderr(contains("missing llm api key"))
        .stdout(is_empty());
}

#[tokio::test]
async fn chat_reports_provider_error_status() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
        .mount(&server)
        .await;

    let config_file = write_temp_config(&format!(
        r#"[llm]
    base_url = "{}"
    api_key = "test-key"
    model = "test-model"
    temperature = 0.7
    "#,
        server.uri()
    ));

    let config_path = config_file.path().to_str().unwrap();

    let mut cmd = Command::cargo_bin("agent-cli-rust").unwrap();

    cmd.arg("chat")
        .arg(config_path)
        .arg("hello")
        .assert()
        .failure()
        .stderr(contains("chat request failed with status 401"))
        .stdout(is_empty());
}

#[tokio::test]
async fn chat_uses_env_api_key_when_config_api_key_is_missing() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("authorization", "Bearer env-key"))
        .and(body_json(serde_json::json!({
            "model": "test-model",
            "temperature": 0.7,
            "messages": [
                {
                    "role": "user",
                    "content": "hello"
                }
            ],
            "stream": false
        })))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{
                "choices": [
                    {
                        "message": {
                            "content": "Hello from mock assistant(env-key)"
                        }
                    }
                ]
            }"#,
        ))
        .mount(&server)
        .await;

    let config_file = write_temp_config(&format!(
        r#"[llm]
    base_url = "{}"
    model = "test-model"
    temperature = 0.7
    "#,
        server.uri()
    ));

    let config_path = config_file.path().to_str().unwrap();

    let mut cmd = Command::cargo_bin("agent-cli-rust").unwrap();

    cmd.env("AGENT_CLI_LLM_API_KEY", "env-key")
        .arg("chat")
        .arg(config_path)
        .arg("hello")
        .assert()
        .success()
        .stdout(contains("Hello from mock assistant(env-key)"))
        .stderr(is_empty());
}
