use assert_cmd::Command;
use predicates::prelude::*;
use predicates::str::contains;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn fetch_prints_response_body_preview() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/hello"))
        .respond_with(ResponseTemplate::new(200).set_body_string("hello from mock server"))
        .mount(&server)
        .await;

    let url = format!("{}/hello", server.uri());

    let mut cmd = Command::cargo_bin("agent-cli-rust").unwrap();

    cmd.arg("fetch")
        .arg(url)
        .assert()
        .success()
        .stdout(contains("hello from mock server"));
}

#[tokio::test]
async fn fetch_returns_error_for_non_success_status() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/hello"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let url = format!("{}/hello", server.uri());

    let mut cmd = Command::cargo_bin("agent-cli-rust").unwrap();

    cmd.arg("fetch")
        .arg(url)
        .assert()
        .failure()
        .stderr(contains("fetch request failed with status 404 Not Found"));
}

#[tokio::test]
async fn fetch_respects_max_chars_argument() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/hello"))
        .respond_with(ResponseTemplate::new(200).set_body_string("abcdef"))
        .mount(&server)
        .await;

    let url = format!("{}/hello", server.uri());

    let mut cmd = Command::cargo_bin("agent-cli-rust").unwrap();

    cmd.arg("fetch")
        .arg("--max-chars")
        .arg("3")
        .arg(url)
        .assert()
        .success()
        .stdout(contains("abc").and(predicate::str::contains("def").not()));
}

#[test]
fn fetch_rejects_zero_max_chars() {
    let mut cmd = Command::cargo_bin("agent-cli-rust").unwrap();

    cmd.arg("fetch")
        .arg("--max-chars")
        .arg("0")
        .arg("http://example.com")
        .assert()
        .failure()
        .stderr(contains("invalid value"));
}
