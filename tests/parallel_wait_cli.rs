use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn parallel_wait_prints_results_in_argument_order() {
    let mut cmd = Command::cargo_bin("agent-cli-rust").unwrap();
    cmd.arg("parallel-wait")
        .arg("2")
        .arg("1")
        .assert()
        .success()
        .stdout(predicate::eq("first_seconds: 2\nsecond_seconds: 1\n"))
        .stderr(predicate::str::is_empty());
}

#[test]
fn parallel_wait_rejects_zero_first_seconds() {
    let mut cmd = Command::cargo_bin("agent-cli-rust").unwrap();
    cmd.arg("parallel-wait")
        .arg("0")
        .arg("1")
        .assert()
        .failure()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains(
            "first_seconds must be between 1 and 60",
        ));
}

#[test]
fn parallel_wait_rejects_zero_second_seconds() {
    let mut cmd = Command::cargo_bin("agent-cli-rust").unwrap();
    cmd.arg("parallel-wait")
        .arg("1")
        .arg("0")
        .assert()
        .failure()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains(
            "second_seconds must be between 1 and 60",
        ));
}
