mod support;

use predicates::prelude::*;
use r#ref::{model::CitationKey, repository::Repository};
use serde_json::Value;
use std::fs;

fn setup() -> (tempfile::TempDir, Repository) {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path()).unwrap();
    let pdf = temp.path().join("fixture.pdf");
    fs::write(&pdf, b"%PDF-1.4\nfixture").unwrap();
    for (key, has_pdf) in [
        ("UsedPdf", true),
        ("UsedNoPdf", false),
        ("UnusedPdf", true),
        ("UnusedNoPdf", false),
    ] {
        repo.add(
            &CitationKey::new(key).unwrap(),
            &support::sample(key, "Doe", Some(2024)),
            has_pdf.then_some(pdf.as_path()),
        )
        .unwrap();
    }
    (temp, repo)
}

fn listed_keys(output: &[u8]) -> Vec<String> {
    let value: Value = serde_json::from_slice(output).unwrap();
    value["result"]["references"]
        .as_array()
        .unwrap()
        .iter()
        .map(|reference| reference["citation_key"].as_str().unwrap().to_owned())
        .collect()
}

// Scenario: the default list and explicit --all return every stored reference.
// Requirements: REQ-021, REQ-079
#[test]
fn default_and_explicit_all_are_equivalent() {
    let (temp, _) = setup();
    let default = support::command(temp.path())
        .args(["list", "--json"])
        .output()
        .unwrap();
    let all = support::command(temp.path())
        .args(["list", "--all", "--json"])
        .output()
        .unwrap();
    assert!(default.status.success());
    assert!(all.status.success());
    assert_eq!(listed_keys(&default.stdout), listed_keys(&all.stdout));
    assert_eq!(listed_keys(&all.stdout).len(), 4);
}

// Scenario: usage filtering from a nested invocation finds citations still farther below it.
// Requirement: REQ-076
#[test]
fn used_and_unused_are_detected_in_nested_scope() {
    let (temp, _) = setup();
    let chapter = temp.path().join("chapters/one");
    fs::create_dir_all(chapter.join("sections")).unwrap();
    fs::write(
        chapter.join("sections/body.tex"),
        "\\cite{UsedPdf, UsedNoPdf}",
    )
    .unwrap();

    let used = support::command(&chapter)
        .args(["list", "--used", "--json"])
        .output()
        .unwrap();
    let unused = support::command(&chapter)
        .args(["list", "--unused", "--json"])
        .output()
        .unwrap();
    assert_eq!(listed_keys(&used.stdout), ["UsedNoPdf", "UsedPdf"]);
    assert_eq!(listed_keys(&unused.stdout), ["UnusedNoPdf", "UnusedPdf"]);
}

// Scenario: PDF filters use source-proof validity, including empty paths as no PDF.
// Requirement: REQ-077
#[test]
fn pdf_and_no_pdf_partition_by_valid_source_proof() {
    let (temp, repo) = setup();
    support::add(
        &repo,
        "EmptyPdf",
        &support::sample("EmptyPdf", "Doe", Some(2024)),
    );
    fs::create_dir_all(repo.pdf_directory().unwrap()).unwrap();
    let yaml = repo
        .reference_path(&CitationKey::new("EmptyPdf").unwrap())
        .join("ref.yaml");
    fs::write(
        &yaml,
        format!(
            "{}pdf_filename: EmptyPdf.pdf\n",
            fs::read_to_string(&yaml).unwrap()
        ),
    )
    .unwrap();
    fs::write(repo.pdf_directory().unwrap().join("EmptyPdf.pdf"), b"").unwrap();
    let pdf = support::command(temp.path())
        .args(["list", "--pdf", "--json"])
        .output()
        .unwrap();
    let no_pdf = support::command(temp.path())
        .args(["list", "--no-pdf", "--json"])
        .output()
        .unwrap();
    assert_eq!(listed_keys(&pdf.stdout), ["UnusedPdf", "UsedPdf"]);
    assert_eq!(
        listed_keys(&no_pdf.stdout),
        ["EmptyPdf", "UnusedNoPdf", "UsedNoPdf"]
    );
}

// Scenario: a usage filter and PDF filter compose using logical AND.
// Requirement: REQ-078
#[test]
fn combined_filters_use_logical_and() {
    let (temp, _) = setup();
    fs::write(temp.path().join("paper.tex"), "UsedPdf UsedNoPdf").unwrap();
    let output = support::command(temp.path())
        .args(["list", "--used", "--pdf", "--json"])
        .output()
        .unwrap();
    assert_eq!(listed_keys(&output.stdout), ["UsedPdf"]);
}

// Scenario: every prohibited pair is rejected with a conflicting-argument error.
// Requirement: REQ-078
#[test]
fn invalid_filter_combinations_are_rejected() {
    let (temp, _) = setup();
    for args in [
        ["list", "--used", "--unused"],
        ["list", "--pdf", "--no-pdf"],
        ["list", "--all", "--used"],
        ["list", "--all", "--unused"],
        ["list", "--all", "--pdf"],
        ["list", "--all", "--no-pdf"],
    ] {
        support::command(temp.path())
            .args(args)
            .assert()
            .failure()
            .stderr(predicate::str::contains("cannot be used with"));
    }
}

// Scenario: a human-readable filtered list with no matches emits no reference blocks.
// Requirement: REQ-079
#[test]
fn no_human_matches_produce_empty_output() {
    let (temp, _) = setup();
    support::command(temp.path())
        .args(["list", "--used"])
        .assert()
        .success()
        .stdout(predicate::eq(""));
}

// Scenario: JSON with no matches preserves the list schema and contains no ANSI escapes.
// Requirement: REQ-079
#[test]
fn no_json_matches_produce_empty_list() {
    let (temp, _) = setup();
    let output = support::command(temp.path())
        .args(["list", "--used", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(!output.stdout.contains(&0x1b));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["result"]["reference_count"], 0);
    assert_eq!(value["result"]["references"], serde_json::json!([]));
}

// Scenario: filtered JSON returns only matching references without changing their shape.
// Requirements: REQ-077, REQ-079
#[test]
fn filtered_json_preserves_reference_schema() {
    let (temp, _) = setup();
    let output = support::command(temp.path())
        .args(["list", "--pdf", "--json"])
        .output()
        .unwrap();
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["result"]["reference_count"], 2);
    assert!(value["result"]["references"][0]["title"].is_string());
    assert!(value["result"]["references"][0]["source_pdf_present"].is_boolean());
}
