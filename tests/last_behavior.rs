mod support;

use predicates::prelude::*;
use r#ref::{model::CitationKey, repository::Repository};
use serde_json::Value;
use std::fs;

fn add_cli(root: &std::path::Path, key: &str) {
    support::command(root)
        .args(["add", "--no-pdf", "--key", key, "--title", key])
        .assert()
        .success();
}

fn set_added_at(root: &std::path::Path, key: &str, timestamp: &str) {
    let path = root.join(format!(".ref/refs/{key}/ref.yaml"));
    let yaml = fs::read_to_string(&path).unwrap();
    let mut value: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
    value.as_mapping_mut().unwrap().insert(
        serde_yaml::Value::String("added_at".into()),
        serde_yaml::Value::String(timestamp.into()),
    );
    fs::write(path, serde_yaml::to_string(&value).unwrap()).unwrap();
}

// Scenario: repository creation metadata drives deterministic recent-reference ordering.
// Requirements: REQ-069, REQ-070
#[test]
fn repository_persists_and_queries_creation_time() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path()).unwrap();
    support::add(&repo, "Zulu", &support::sample("Zulu", "Doe", Some(2024)));
    support::add(&repo, "Alpha", &support::sample("Alpha", "Doe", Some(2024)));
    support::add(&repo, "Older", &support::sample("Older", "Doe", Some(2024)));
    set_added_at(temp.path(), "Zulu", "2026-09-07T15:42:31.000000000Z");
    set_added_at(temp.path(), "Alpha", "2026-09-07T15:42:31.000000000Z");
    set_added_at(temp.path(), "Older", "2020-01-01T00:00:00.000000000Z");

    let recent = repo.recent_references(10).unwrap();
    assert_eq!(
        recent.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(),
        ["Alpha", "Zulu", "Older"]
    );
    assert!(recent.iter().all(|r| r.added_at.is_some()));
}

// Scenario: legacy YAML remains loadable but has no fabricated creation time.
// Requirements: REQ-004, REQ-069, REQ-070
#[test]
fn legacy_references_are_valid_and_excluded() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path()).unwrap();
    let key = CitationKey::new("Legacy").unwrap();
    fs::create_dir(repo.reference_path(&key)).unwrap();
    fs::write(
        repo.reference_path(&key).join("ref.yaml"),
        serde_yaml::to_string(&support::sample("Legacy", "Doe", Some(2024))).unwrap(),
    )
    .unwrap();

    assert_eq!(repo.load_reference(&key).unwrap().added_at, None);
    assert!(repo.recent_references(1).unwrap().is_empty());
}

// Scenario: plain last output is minimal, newest-first, bounded, and allows short results.
// Requirements: REQ-070, REQ-071
#[test]
fn plain_last_supports_default_and_number() {
    let temp = tempfile::tempdir().unwrap();
    support::command(temp.path()).arg("init").assert().success();
    for key in ["First", "Second", "Third"] {
        add_cli(temp.path(), key);
    }
    set_added_at(temp.path(), "First", "2024-01-01T00:00:00.000000000Z");
    set_added_at(temp.path(), "Second", "2025-01-01T00:00:00.000000000Z");
    set_added_at(temp.path(), "Third", "2026-01-01T00:00:00.000000000Z");

    support::command(temp.path())
        .arg("last")
        .assert()
        .success()
        .stdout("Third\n")
        .stderr("");
    support::command(temp.path())
        .args(["last", "-n", "10"])
        .assert()
        .success()
        .stdout("Third\nSecond\nFirst\n")
        .stderr("");
}

// Scenario: empty and legacy-only repositories produce an empty successful query.
// Requirements: REQ-069, REQ-070, REQ-071
#[test]
fn empty_last_is_silent_success() {
    let temp = tempfile::tempdir().unwrap();
    support::command(temp.path()).arg("init").assert().success();
    support::command(temp.path())
        .arg("last")
        .assert()
        .success()
        .stdout("")
        .stderr("");
}

// Scenario: rename and attachment preserve recency and expose the current key.
// Requirements: REQ-019, REQ-029, REQ-069, REQ-070
#[test]
fn rename_and_attach_do_not_change_recency() {
    let temp = tempfile::tempdir().unwrap();
    support::command(temp.path()).arg("init").assert().success();
    add_cli(temp.path(), "Older");
    add_cli(temp.path(), "Newest");
    set_added_at(temp.path(), "Older", "2024-01-01T00:00:00.000000000Z");
    set_added_at(temp.path(), "Newest", "2025-01-01T00:00:00.000000000Z");
    fs::write(temp.path().join("paper.pdf"), b"%PDF-1.4").unwrap();
    support::command(temp.path())
        .args(["attach", "Older", "paper.pdf"])
        .assert()
        .success();
    support::command(temp.path())
        .args(["rename", "Older", "Renamed"])
        .assert()
        .success();

    support::command(temp.path())
        .args(["last", "-n", "2"])
        .assert()
        .success()
        .stdout("Newest\nRenamed\n");
    assert!(
        fs::read_to_string(temp.path().join(".ref/refs/Renamed/ref.yaml"))
            .unwrap()
            .contains("2024-01-01T00:00:00.000000000Z")
    );
}

// Scenario: last JSON uses the common envelope and key-only result objects.
// Requirements: REQ-067, REQ-072
#[test]
fn last_json_uses_common_output() {
    let temp = tempfile::tempdir().unwrap();
    support::command(temp.path()).arg("init").assert().success();
    add_cli(temp.path(), "Newest");
    let output = support::command(temp.path())
        .args(["last", "-n", "3", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["command"], "last");
    assert_eq!(json["result"]["references"][0]["citation_key"], "Newest");
    assert!(json["result"]["references"][0].get("added_at").is_none());
}

// Scenario: last count rejects zero, negative, and non-numeric values in Clap.
// Requirement: REQ-070
#[test]
fn last_rejects_invalid_numbers() {
    let temp = tempfile::tempdir().unwrap();
    for value in ["0", "-1", "invalid"] {
        support::command(temp.path())
            .args(["last", "-n", value])
            .assert()
            .failure()
            .stderr(predicate::str::contains("invalid value"));
    }
}

// Scenario: successfully imported entries receive creation metadata and failed ones do not exist.
// Requirements: REQ-041, REQ-069, REQ-070
#[test]
fn imported_references_participate_in_last() {
    let temp = tempfile::tempdir().unwrap();
    support::command(temp.path()).arg("init").assert().success();
    fs::write(
        temp.path().join("references.bib"),
        "@article{Good, title={Good}, year={2024}}\n@article{Bad, year={2024}}\n",
    )
    .unwrap();
    support::command(temp.path())
        .args(["import", "references.bib"])
        .assert()
        .failure();
    support::command(temp.path())
        .args(["last", "--number", "3"])
        .assert()
        .success()
        .stdout("Good\n");
    assert!(!temp.path().join(".ref/refs/Bad").exists());
}
