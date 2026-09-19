mod support;

use predicates::prelude::*;
use r#ref::{model::CitationKey, repository::Repository};
use std::fs;

// Scenario: init records the default without eagerly creating PDF storage.
// Requirements: REQ-080, REQ-081, REQ-091
#[test]
fn init_configures_lazy_default_pdf_directory() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path()).unwrap();
    assert_eq!(
        fs::read_to_string(repo.root().join("config.yaml")).unwrap(),
        "version: 1\npdf_directories:\n  - source\npdf_hashing: true\n"
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

fn set_pdf_filename(repo: &Repository, key: &CitationKey, filename: &str) {
    let yaml = repo.reference_path(key).join("ref.yaml");
    let mut text = fs::read_to_string(&yaml).unwrap();
    text.push_str(&format!("pdf_filename: {filename}\n"));
    fs::write(yaml, text).unwrap();
}

// Scenario: lookup respects configured order, supports relative and absolute
// directories, and then falls back to a path relative to .ref.
// Requirements: REQ-080, REQ-082, REQ-085
#[test]
fn ordered_lookup_and_relative_fallback() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path()).unwrap();
    let archive = temp.path().join("archive");
    fs::create_dir_all(&archive).unwrap();
    fs::write(
        repo.root().join("config.yaml"),
        format!(
            "version: 1\npdf_directories:\n  - primary\n  - {}\n",
            archive.display()
        ),
    )
    .unwrap();
    let key = CitationKey::new("Einstein").unwrap();
    support::add(
        &repo,
        key.as_str(),
        &support::sample("Theory", "Einstein", Some(1920)),
    );
    set_pdf_filename(&repo, &key, "legacy/Einstein.pdf");

    fs::create_dir_all(archive.join("legacy")).unwrap();
    fs::write(archive.join("legacy/Einstein.pdf"), b"archive").unwrap();
    fs::create_dir_all(repo.root().join("primary/legacy")).unwrap();
    fs::write(repo.root().join("primary/legacy/Einstein.pdf"), b"primary").unwrap();
    assert_eq!(
        repo.source_pdf_path(&key).unwrap(),
        repo.root().join("primary/legacy/Einstein.pdf")
    );

    fs::remove_file(repo.root().join("primary/legacy/Einstein.pdf")).unwrap();
    assert_eq!(
        repo.source_pdf_path(&key).unwrap(),
        archive.join("legacy/Einstein.pdf")
    );
    fs::remove_file(archive.join("legacy/Einstein.pdf")).unwrap();
    fs::create_dir_all(repo.root().join("legacy")).unwrap();
    fs::write(repo.root().join("legacy/Einstein.pdf"), b"fallback").unwrap();
    assert_eq!(
        repo.source_pdf_path(&key).unwrap(),
        repo.root().join("legacy/Einstein.pdf")
    );
}

// Scenario: an absolute metadata path bypasses configured directories and is
// external, just like a direct relative fallback, so mutation leaves it alone.
// Requirements: REQ-082, REQ-086, REQ-087
#[test]
fn direct_paths_resolve_but_are_never_mutated() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path()).unwrap();
    let external = temp.path().join("external.pdf");
    fs::write(&external, b"external").unwrap();
    let old = CitationKey::new("External").unwrap();
    support::add(
        &repo,
        old.as_str(),
        &support::sample("External", "Doe", Some(2024)),
    );
    set_pdf_filename(&repo, &old, external.to_str().unwrap());
    // Even a same-named file in configured storage must be skipped for an
    // absolute pdf_filename.
    fs::create_dir(repo.root().join("source")).unwrap();
    fs::write(repo.root().join("source/external.pdf"), b"configured").unwrap();
    assert_eq!(repo.source_pdf_path(&old).unwrap(), external);

    let new = CitationKey::new("Renamed").unwrap();
    repo.rename(&old, &new).unwrap();
    assert_eq!(fs::read(&external).unwrap(), b"external");
    repo.remove(&new).unwrap();
    assert_eq!(fs::read(&external).unwrap(), b"external");

    let relative = repo.root().join("legacy/relative.pdf");
    fs::create_dir_all(relative.parent().unwrap()).unwrap();
    fs::write(&relative, b"relative").unwrap();
    let old = CitationKey::new("RelativeExternal").unwrap();
    support::add(
        &repo,
        old.as_str(),
        &support::sample("Relative", "Doe", Some(2024)),
    );
    set_pdf_filename(&repo, &old, "legacy/relative.pdf");
    assert_eq!(repo.source_pdf_path(&old).unwrap(), relative);
    let new = CitationKey::new("RelativeRenamed").unwrap();
    repo.rename(&old, &new).unwrap();
    repo.remove(&new).unwrap();
    assert_eq!(fs::read(relative).unwrap(), b"relative");
}

// Scenario: relative and absolute PDF directories resolve from the documented bases.
// Requirements: REQ-080, REQ-082
#[test]
fn relative_and_absolute_directories_are_resolved() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path()).unwrap();
    fs::write(
        repo.root().join("config.yaml"),
        "version: 1\npdf_directories:\n  - assets/pdfs\n",
    )
    .unwrap();
    assert_eq!(
        repo.pdf_directory().unwrap(),
        repo.root().join("assets/pdfs")
    );
    let external = temp.path().join("external");
    fs::write(
        repo.root().join("config.yaml"),
        format!("version: 1\npdf_directories:\n  - {}\n", external.display()),
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
