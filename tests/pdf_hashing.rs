mod support;

use predicates::prelude::*;
use r#ref::{doctor, model::CitationKey, repository::Repository};
use std::fs;

const PDF: &[u8] = b"%PDF-1.4\ndeterministic fixture\n";
const DIGEST: &str = "9b37f081f4d0635f99b87e4ddfd70131586bbf3e140834a9a0da954a964e75bc";

fn repo_with_pdf() -> (tempfile::TempDir, Repository, CitationKey) {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path()).unwrap();
    let source = temp.path().join("input.pdf");
    fs::write(&source, PDF).unwrap();
    let key = CitationKey::new("Example2024").unwrap();
    repo.add(
        &key,
        &support::sample("Example", "Smith", Some(2024)),
        Some(&source),
    )
    .unwrap();
    (temp, repo, key)
}

// Scenario: absent hashing configuration enables hashing, while invalid scalar types fail clearly.
// Requirements: REQ-091
#[test]
fn configuration_default_and_validation() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path()).unwrap();
    fs::write(repo.root().join("config.yaml"), "version: 1\n").unwrap();
    assert!(repo.pdf_hashing().unwrap());
    fs::write(
        repo.root().join("config.yaml"),
        "version: 1\npdf_hashing: perhaps\n",
    )
    .unwrap();
    assert!(repo
        .pdf_hashing()
        .unwrap_err()
        .to_string()
        .contains("invalid"));
}

// Scenario: adding a PDF stores its correct lowercase SHA-256 digest.
// Requirements: REQ-092, REQ-093
#[test]
fn add_stores_correct_hash() {
    let (_temp, repo, key) = repo_with_pdf();
    let text = fs::read_to_string(repo.reference_path(&key).join("ref.yaml")).unwrap();
    assert!(text.contains(&format!("pdf_sha256: {DIGEST}")), "{text}");
}

// Scenario: disabled hashing neither creates hashes nor reports absent hash metadata.
// Requirements: REQ-091, REQ-094
#[test]
fn disabled_add_and_doctor_skip_hashing() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path()).unwrap();
    fs::write(
        repo.root().join("config.yaml"),
        "version: 1\npdf_hashing: false\n",
    )
    .unwrap();
    let source = temp.path().join("x.pdf");
    fs::write(&source, PDF).unwrap();
    let key = CitationKey::new("Disabled").unwrap();
    repo.add(
        &key,
        &support::sample("Example", "Smith", Some(2024)),
        Some(&source),
    )
    .unwrap();
    let text = fs::read_to_string(repo.reference_path(&key).join("ref.yaml")).unwrap();
    assert!(!text.contains("pdf_sha256"));
    assert!(doctor::inspect(&repo)
        .unwrap()
        .diagnostics
        .iter()
        .all(|d| !matches!(
            d,
            doctor::DoctorDiagnostic::MissingPdfHash { .. }
                | doctor::DoctorDiagnostic::PdfHashMismatch { .. }
        )));
}

// Scenario: doctor accepts a matching hash and detects a changed PDF as an error.
// Requirements: REQ-094
#[test]
fn doctor_verifies_matching_and_mismatching_hashes() {
    let (_temp, repo, key) = repo_with_pdf();
    assert!(doctor::inspect(&repo)
        .unwrap()
        .diagnostics
        .iter()
        .all(|d| !matches!(
            d,
            doctor::DoctorDiagnostic::PdfHashMismatch { .. }
                | doctor::DoctorDiagnostic::MissingPdfHash { .. }
        )));
    fs::write(repo.source_pdf_path(&key).unwrap(), b"changed").unwrap();
    assert!(doctor::inspect(&repo)
        .unwrap()
        .diagnostics
        .iter()
        .any(
            |d| matches!(d, doctor::DoctorDiagnostic::PdfHashMismatch { .. })
                && d.severity() == doctor::DiagnosticSeverity::Error
        ));
}

// Scenario: doctor gives an actionable warning for backward-compatible unhashed PDF metadata.
// Requirements: REQ-092, REQ-094
#[test]
fn doctor_reports_missing_hash() {
    let (_temp, repo, key) = repo_with_pdf();
    let yaml = repo.reference_path(&key).join("ref.yaml");
    let text = fs::read_to_string(&yaml)
        .unwrap()
        .lines()
        .filter(|l| !l.starts_with("pdf_sha256:"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(yaml, text).unwrap();
    let d = doctor::inspect(&repo)
        .unwrap()
        .diagnostics
        .into_iter()
        .find(|d| matches!(d, doctor::DoctorDiagnostic::MissingPdfHash { .. }))
        .unwrap();
    assert!(d.message().contains("ref hash"));
}

// Scenario: hash refresh updates available PDFs, skips unavailable ones, and dry-run is immutable.
// Requirements: REQ-093
#[test]
fn refresh_dry_run_and_unavailable_pdf_are_safe() {
    let (_temp, repo, key) = repo_with_pdf();
    let yaml = repo.reference_path(&key).join("ref.yaml");
    let before = fs::read_to_string(&yaml).unwrap();
    fs::write(repo.source_pdf_path(&key).unwrap(), b"changed").unwrap();
    let missing = CitationKey::new("Missing").unwrap();
    support::add(
        &repo,
        "Missing",
        &support::sample("Missing", "Smith", Some(2024)),
    );
    let (updated, skipped) = repo.refresh_pdf_hashes(true).unwrap();
    assert_eq!(updated, vec![key.clone()]);
    assert_eq!(skipped, vec![missing]);
    assert_eq!(fs::read_to_string(&yaml).unwrap(), before);
    repo.refresh_pdf_hashes(false).unwrap();
    assert_ne!(fs::read_to_string(yaml).unwrap(), before);
}

// Scenario: hash rejects disabled configuration without changing metadata.
// Requirements: REQ-091, REQ-093
#[test]
fn hash_command_rejects_disabled_configuration() {
    let (temp, repo, key) = repo_with_pdf();
    fs::write(
        repo.root().join("config.yaml"),
        "version: 1\npdf_hashing: false\n",
    )
    .unwrap();
    let yaml = repo.reference_path(&key).join("ref.yaml");
    let before = fs::read_to_string(&yaml).unwrap();
    support::command(temp.path())
        .arg("hash")
        .assert()
        .failure()
        .stderr(predicate::str::contains("hashing is disabled"));
    assert_eq!(fs::read_to_string(yaml).unwrap(), before);
}

// Scenario: hash JSON output is a valid central envelope with dry-run results and skipped keys.
// Requirements: REQ-093, REQ-095
#[test]
fn hash_json_output_is_machine_readable() {
    let (temp, repo, key) = repo_with_pdf();
    let yaml = repo.reference_path(&key).join("ref.yaml");
    let text = fs::read_to_string(&yaml)
        .unwrap()
        .lines()
        .filter(|l| !l.starts_with("pdf_sha256:"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(yaml, text).unwrap();
    let output = support::command(temp.path())
        .args(["hash", "--dry-run", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["command"], "hash");
    assert_eq!(value["result"]["dry_run"], true);
    assert_eq!(
        value["result"]["updated_references"][0]["citation_key"],
        "Example2024"
    );
}
