mod support;

use predicates::prelude::*;
use r#ref::repository::Repository;
use std::fs;

fn setup() -> (tempfile::TempDir, Repository) {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path()).unwrap();
    for key in [
        "RootReference",
        "EEGReference",
        "UnusedReference",
        "EEGReferenceModel",
    ] {
        support::add(
            &repo,
            key,
            &support::sample(&format!("Title {key}"), "Doe", Some(2024)),
        );
    }
    (temp, repo)
}

// Scenario: root scan excludes bibliographies repository git and binary content.
// Requirement: REQ-032
#[test]
fn root_scan_excludes_bibliographies_repository_git_and_binary_content() {
    let (temp, _) = setup();
    fs::write(temp.path().join("thesis.tex"), "% \\cite{RootReference}\n").unwrap();
    fs::write(
        temp.path().join("references.bib"),
        "EEGReference UnusedReference EEGReferenceModel",
    )
    .unwrap();
    fs::create_dir(temp.path().join(".git")).unwrap();
    fs::write(temp.path().join(".git/history"), "UnusedReference").unwrap();
    fs::write(temp.path().join("binary.dat"), b"\0EEGReference").unwrap();
    support::command(temp.path())
        .args(["clean", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Used:          1"))
        .stdout(predicate::str::contains("Unused:        3"));
    assert!(temp.path().join(".ref/refs/UnusedReference").is_dir());
}

// Scenario: nested scan never searches parents or siblings and honors boundaries.
// Requirements: REQ-031, REQ-033
#[test]
fn nested_scan_never_searches_parents_or_siblings_and_honors_boundaries() {
    let (temp, _) = setup();
    let eeg = temp.path().join("chapters/eeg");
    fs::create_dir_all(&eeg).unwrap();
    fs::create_dir_all(temp.path().join("chapters/recommender")).unwrap();
    fs::write(temp.path().join("root.tex"), "RootReference").unwrap();
    fs::write(
        temp.path().join("chapters/recommender/sibling.tex"),
        "UnusedReference",
    )
    .unwrap();
    fs::write(eeg.join("eeg.tex"), "EEGReferenceModel").unwrap();
    support::command(&eeg)
        .args(["clean", "-n"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Used:          1"))
        .stdout(predicate::str::contains("Note: files outside"))
        .stdout(predicate::str::contains("RootReference"));
}

// Scenario: path scopes union and cannot escape invocation directory.
// Requirement: REQ-034
#[test]
fn path_scopes_union_and_cannot_escape_invocation_directory() {
    let (temp, _) = setup();
    fs::create_dir(temp.path().join("chapters")).unwrap();
    fs::write(temp.path().join("intro.tex"), "RootReference").unwrap();
    fs::write(temp.path().join("chapters/eeg.tex"), "EEGReference").unwrap();
    fs::write(temp.path().join("other.tex"), "UnusedReference").unwrap();
    support::command(temp.path())
        .args(["clean", "-n", "intro.tex", "chapters"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Used:          2"));
    support::command(&temp.path().join("chapters"))
        .args(["clean", "-n", "../"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("must remain within"));
}

// Scenario: yes removes only unused references and dry run wins over yes.
// Requirement: REQ-035
#[test]
fn yes_removes_only_unused_references_and_dry_run_wins_over_yes() {
    let (temp, _) = setup();
    fs::write(
        temp.path().join("thesis.tex"),
        "RootReference EEGReference EEGReferenceModel",
    )
    .unwrap();
    support::command(temp.path())
        .args(["clean", "--dry-run", "--yes"])
        .assert()
        .success();
    assert!(temp.path().join(".ref/refs/UnusedReference").is_dir());
    support::command(temp.path())
        .args(["clean", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Removed 1 unused references"));
    assert!(!temp.path().join(".ref/refs/UnusedReference").exists());
    assert!(temp.path().join(".ref/refs/RootReference").is_dir());
}

// Scenario: empty text scope is prominent.
// Requirement: REQ-036
#[test]
fn empty_text_scope_is_prominent() {
    let (temp, _) = setup();
    fs::write(temp.path().join("only.pdf"), b"%PDF").unwrap();
    support::command(temp.path())
        .args(["clean", "-n", "only.pdf"])
        .assert()
        .success()
        .stdout(predicate::str::contains("no eligible text files"));
}

// Scenario: confirmation can cancel or remove the complete reference directory.
// Requirement: REQ-035
#[test]
fn confirmation_can_cancel_or_remove_the_complete_reference_directory() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path()).unwrap();
    let key = r#ref::model::CitationKey::new("UnusedReference").unwrap();
    let pdf = temp.path().join("source.pdf");
    fs::write(&pdf, b"%PDF").unwrap();
    repo.add(
        &key,
        &support::sample("Unused title", "Doe", Some(2024)),
        Some(&pdf),
    )
    .unwrap();
    fs::write(temp.path().join("source.tex"), "No citations here.").unwrap();

    support::command(temp.path())
        .arg("clean")
        .write_stdin("\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("Cleanup cancelled"));
    assert!(repo.reference_path(&key).join("paper.pdf").is_file());

    support::command(temp.path())
        .arg("clean")
        .write_stdin("yes\n")
        .assert()
        .success();
    assert!(!repo.reference_path(&key).exists());
}
