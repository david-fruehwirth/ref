mod support;

use predicates::prelude::*;
use r#ref::{model::CitationKey, repository::Repository};
use std::fs;

// Scenario: init records the default without eagerly creating PDF storage.
// Requirements: REQ-080, REQ-081
#[test]
fn init_configures_lazy_default_pdf_directory() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path()).unwrap();
    assert_eq!(
        fs::read_to_string(repo.root().join("config.yaml")).unwrap(),
        "version: 1\npdf_directory: source\n"
    );
    assert!(!repo.root().join("source").exists());
}

// Scenario: omitted configuration remains compatible and resolves to source.
// Requirement: REQ-080
#[test]
fn absent_pdf_directory_uses_source() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path()).unwrap();
    fs::write(repo.root().join("config.yaml"), "version: 1\n").unwrap();
    assert_eq!(repo.pdf_directory().unwrap(), repo.root().join("source"));
}

// Scenario: relative and absolute PDF directories resolve from the documented bases.
// Requirements: REQ-080, REQ-082
#[test]
fn relative_and_absolute_directories_are_resolved() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path()).unwrap();
    fs::write(
        repo.root().join("config.yaml"),
        "version: 1\npdf_directory: assets/pdfs\n",
    )
    .unwrap();
    assert_eq!(
        repo.pdf_directory().unwrap(),
        repo.root().join("assets/pdfs")
    );
    let external = temp.path().join("external");
    fs::write(
        repo.root().join("config.yaml"),
        format!("version: 1\npdf_directory: {}\n", external.display()),
    )
    .unwrap();
    assert_eq!(repo.pdf_directory().unwrap(), external);
    let input = temp.path().join("absolute-input.pdf");
    fs::write(&input, b"%PDF absolute").unwrap();
    repo.add(
        &CitationKey::new("Absolute").unwrap(),
        &support::sample("Absolute", "Doe", Some(2024)),
        Some(&input),
    )
    .unwrap();
    assert_eq!(
        fs::read(external.join("Absolute.pdf")).unwrap(),
        b"%PDF absolute"
    );
}

// Scenario: add names the managed PDF after its key and stores only that basename.
// Requirements: REQ-083, REQ-084
#[test]
fn add_uses_configured_name_and_metadata_basename() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path()).unwrap();
    let input = temp.path().join("paper-final.pdf");
    fs::write(&input, b"%PDF fixture").unwrap();
    let key = CitationKey::new("Einstein1920RelativeTheory").unwrap();
    repo.add(
        &key,
        &support::sample("Relative Theory", "Einstein", Some(1920)),
        Some(&input),
    )
    .unwrap();
    assert_eq!(
        fs::read(repo.root().join("source/Einstein1920RelativeTheory.pdf")).unwrap(),
        b"%PDF fixture"
    );
    let yaml = fs::read_to_string(repo.reference_path(&key).join("ref.yaml")).unwrap();
    assert!(yaml.contains("pdf_filename: Einstein1920RelativeTheory.pdf"));
    assert!(!yaml.contains("paper-final"));
}

// Scenario: invalid configured storage fails actionably while a missing directory is created on write.
// Requirements: REQ-081, REQ-085
#[test]
fn configured_directory_validation_and_lazy_creation() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path()).unwrap();
    fs::write(repo.root().join("source"), "not a directory").unwrap();
    assert!(repo
        .validate_structure()
        .unwrap_err()
        .to_string()
        .contains("not a directory"));
    fs::remove_file(repo.root().join("source")).unwrap();
    let input = temp.path().join("input.pdf");
    fs::write(&input, b"%PDF").unwrap();
    repo.add(
        &CitationKey::new("Lazy").unwrap(),
        &support::sample("Lazy", "Doe", Some(2024)),
        Some(&input),
    )
    .unwrap();
    assert!(repo.root().join("source/Lazy.pdf").is_file());
}

// Scenario: rename and remove mutate only the configured managed PDF.
// Requirements: REQ-086, REQ-087
#[test]
fn rename_and_remove_follow_configured_pdf() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path()).unwrap();
    let input = temp.path().join("input.pdf");
    fs::write(&input, b"%PDF").unwrap();
    let old = CitationKey::new("Old").unwrap();
    let new = CitationKey::new("New").unwrap();
    repo.add(
        &old,
        &support::sample("Title", "Doe", Some(2024)),
        Some(&input),
    )
    .unwrap();
    repo.rename(&old, &new).unwrap();
    assert!(!repo.root().join("source/Old.pdf").exists());
    assert!(repo.root().join("source/New.pdf").is_file());
    assert!(
        fs::read_to_string(repo.reference_path(&new).join("ref.yaml"))
            .unwrap()
            .contains("pdf_filename: New.pdf")
    );
    repo.remove(&new).unwrap();
    assert!(!repo.root().join("source/New.pdf").exists());
}

// Scenario: migration dry-run is non-mutating and execution moves and records legacy PDFs.
// Requirements: REQ-088, REQ-089
#[test]
fn migration_dry_run_then_moves_safely() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path()).unwrap();
    let key = CitationKey::new("Legacy").unwrap();
    support::add(
        &repo,
        key.as_str(),
        &support::sample("Legacy", "Doe", Some(2020)),
    );
    let legacy = repo.reference_path(&key).join("paper.pdf");
    fs::write(&legacy, b"legacy").unwrap();
    let plan = repo.migrate_pdfs(true).unwrap();
    assert_eq!(plan.len(), 1);
    assert!(legacy.exists());
    assert!(!repo.root().join("source").exists());
    repo.migrate_pdfs(false).unwrap();
    assert!(!legacy.exists());
    assert_eq!(
        fs::read(repo.root().join("source/Legacy.pdf")).unwrap(),
        b"legacy"
    );
    assert!(
        fs::read_to_string(repo.reference_path(&key).join("ref.yaml"))
            .unwrap()
            .contains("pdf_filename: Legacy.pdf")
    );
}

// Scenario: a migration collision fails before deleting or moving the legacy PDF.
// Requirement: REQ-089
#[test]
fn failed_migration_preserves_legacy_pdf() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path()).unwrap();
    let key = CitationKey::new("Legacy").unwrap();
    support::add(
        &repo,
        key.as_str(),
        &support::sample("Legacy", "Doe", Some(2020)),
    );
    let legacy = repo.reference_path(&key).join("paper.pdf");
    fs::write(&legacy, b"original").unwrap();
    fs::create_dir(repo.root().join("source")).unwrap();
    fs::write(repo.root().join("source/Legacy.pdf"), b"unmanaged").unwrap();
    assert!(repo
        .migrate_pdfs(false)
        .unwrap_err()
        .to_string()
        .contains("already exists"));
    assert_eq!(fs::read(legacy).unwrap(), b"original");
    assert_eq!(
        fs::read(repo.root().join("source/Legacy.pdf")).unwrap(),
        b"unmanaged"
    );
}

// Scenario: legacy layout fails clearly until the explicit migration command is used.
// Requirements: REQ-088, REQ-090
#[test]
fn legacy_repository_has_actionable_error() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path()).unwrap();
    let key = CitationKey::new("Legacy").unwrap();
    support::add(
        &repo,
        key.as_str(),
        &support::sample("Legacy", "Doe", Some(2020)),
    );
    fs::write(repo.reference_path(&key).join("paper.pdf"), b"legacy").unwrap();
    support::command(temp.path())
        .args(["show", "Legacy"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("migrate-pdfs"));
}
