mod support;
use r#ref::repository::Repository;
use serde_json::Value;
use support::{add, command, sample};
use tempfile::TempDir;

fn repository() -> (TempDir, Repository) {
    let temp = TempDir::new().unwrap();
    let repo = Repository::init(temp.path()).unwrap();
    (temp, repo)
}
fn json(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).expect("output is one JSON document")
}

// Scenario: list and search use common envelope.
// Requirements: REQ-021, REQ-022, REQ-067
#[test]
fn list_and_search_use_common_envelope() {
    let (temp, repo) = repository();
    add(
        &repo,
        "Smith2024Attention",
        &sample("Attention Models", "Smith", Some(2024)),
    );
    for args in [
        vec!["list", "--json"],
        vec!["--json", "search", "attention"],
    ] {
        let output = command(temp.path()).args(args).output().unwrap();
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        let value = json(&output.stdout);
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["operation_status"], "success");
        assert!(value["result"].is_object());
    }
}
// Scenario: missing reference is structured stderr only.
// Requirements: REQ-023, REQ-066, REQ-067
#[test]
fn missing_reference_is_structured_stderr_only() {
    let (temp, _) = repository();
    let output = command(temp.path())
        .args(["show", "MissingKey", "--json"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let value = json(&output.stderr);
    assert_eq!(value["operation_status"], "failure");
    assert_eq!(value["error"]["error_code"], "reference_not_found");
    assert_eq!(value["error"]["citation_key"], "MissingKey");
}
// Scenario: json remove requires explicit confirmation.
// Requirement: REQ-068
#[test]
fn json_remove_requires_explicit_confirmation() {
    let (temp, repo) = repository();
    add(
        &repo,
        "Smith2024Attention",
        &sample("Attention Models", "Smith", Some(2024)),
    );
    let output = command(temp.path())
        .args(["remove", "Smith2024Attention", "--json"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(
        json(&output.stderr)["error"]["error_code"],
        "confirmation_required"
    );
}

// Scenario: clean warnings stay structured and do not contaminate its JSON document.
// Requirements: REQ-036, REQ-066, REQ-067
#[test]
fn json_clean_reports_empty_scope_in_the_common_envelope() {
    let (temp, repo) = repository();
    add(
        &repo,
        "Smith2024Attention",
        &sample("Attention Models", "Smith", Some(2024)),
    );
    std::fs::write(temp.path().join("only.pdf"), b"%PDF").unwrap();
    let output = command(temp.path())
        .args(["clean", "--dry-run", "only.pdf", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let value = json(&output.stdout);
    assert_eq!(value["command"], "clean");
    assert_eq!(value["warnings"][0]["warning_code"], "empty_clean_scope");
}
