use assert_cmd::Command;
use predicates::str::contains;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

use std::io::Write;

#[tokio::test]
async fn chat_prints_assistant_response_from_mock_server() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
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

    let mut config_file = tempfile::NamedTempFile::new().unwrap(); //要临时写一个config，就临时创建一个文件用完就drop

    write!(
        config_file,
        r#"[llm]
      base_url = "{}"
      api_key = "test-key"
      model = "test-model"
      temperature = 0.7
      "#,
        server.uri()
    )
    .unwrap();

    let config_path = config_file.path().to_str().unwrap();

    let mut cmd = Command::cargo_bin("agent-cli-rust").unwrap();

    cmd.arg("chat")
        .arg(config_path)
        .arg("hello")
        .assert()
        .success()
        .stdout(contains("Hello from mock assistant"));
}

#[test]
fn chat_rejects_missing_api_key() {
    let mut config_file = tempfile::NamedTempFile::new().unwrap();

    write!(
        config_file,
        r#"[llm]
      base_url = "https://127.0.0.1:1"
      model = "test-model"
      temperature = 0.7
      "#,
    )
    .unwrap();

    let config_path = config_file.path().to_str().unwrap();

    let mut cmd = Command::cargo_bin("agent-cli-rust").unwrap();

    cmd.arg("chat")
        .arg(config_path)
        .arg("hello")
        .assert()
        .failure()
        .stderr(contains("missing llm api key"));
}
