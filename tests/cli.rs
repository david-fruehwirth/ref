use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;

fn cmd(dir: &std::path::Path) -> Command {
    let mut c = Command::cargo_bin("ref").unwrap();
    c.current_dir(dir);
    c
}

// Scenario: no pdf workflow and export.
// Requirements: REQ-016, REQ-050, REQ-065
#[test]
fn no_pdf_workflow_and_export() {
    let t = tempfile::tempdir().unwrap();
    cmd(t.path()).arg("init").assert().success();
    cmd(t.path())
        .args([
            "add",
            "--no-pdf",
            "--key",
            "smith2024",
            "--title",
            "An Example",
            "--author",
            "Jane, Smith",
            "--year",
            "2024",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added smith2024"));
    assert!(t.path().join(".ref/refs/smith2024/ref.yaml").is_file());
    cmd(t.path())
        .args(["search", "SMITH"])
        .assert()
        .success()
        .stdout(predicate::str::contains("An Example"));
    cmd(t.path())
        .arg("export")
        .assert()
        .success()
        .stdout(predicate::str::contains("@article{smith2024"));
    cmd(t.path())
        .args(["rename", "smith2024", "smith2025"])
        .assert()
        .success();
    cmd(t.path())
        .args(["remove", "smith2025", "--yes"])
        .assert()
        .success();
    assert!(!t.path().join(".ref/refs/smith2025").exists());
}

// Scenario: pdf is copied and duplicate rejected.
// Requirements: REQ-006, REQ-015
#[test]
fn pdf_is_copied_and_duplicate_rejected() {
    let t = tempfile::tempdir().unwrap();
    cmd(t.path()).arg("init").assert().success();
    fs::write(t.path().join("source.PDF"), b"pdf").unwrap();
    let args = [
        "add",
        "source.PDF",
        "--key",
        "x1",
        "--title",
        "X",
        "--author",
        "A, B",
    ];
    cmd(t.path()).args(args).assert().success();
    assert_eq!(
        fs::read(t.path().join(".ref/source/x1.pdf")).unwrap(),
        b"pdf"
    );
    cmd(t.path())
        .args(args)
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
}

// Scenario: doctor strict treats warning as failure.
// Requirements: REQ-053, REQ-056
#[test]
fn doctor_strict_treats_warning_as_failure() {
    let t = tempfile::tempdir().unwrap();
    cmd(t.path()).arg("init").assert().success();
    cmd(t.path())
        .args([
            "add", "--no-pdf", "--key", "x", "--title", "X", "--author", "A, B",
        ])
        .assert()
        .success();
    cmd(t.path())
        .arg("doctor")
        .assert()
        .success()
        .stdout(predicate::str::contains("x: source PDF missing"))
        .stdout(predicate::str::contains(
            "0 / 1 references have source PDFs",
        ));
    cmd(t.path()).args(["doctor", "--strict"]).assert().code(1);
}

// Scenario: attach copies without overwriting and resolves doctor warning.
// Requirements: REQ-019, REQ-053
#[test]
fn attach_copies_without_overwriting_and_resolves_doctor_warning() {
    let t = tempfile::tempdir().unwrap();
    cmd(t.path()).arg("init").assert().success();
    cmd(t.path())
        .args([
            "add", "--no-pdf", "--key", "x", "--title", "X", "--author", "A, B", "--year", "2024",
        ])
        .assert()
        .success();
    let source = t.path().join("source.pdf");
    fs::write(&source, b"first source").unwrap();
    cmd(t.path())
        .args(["attach", "x", "source.pdf"])
        .assert()
        .success();
    assert_eq!(fs::read(&source).unwrap(), b"first source");
    cmd(t.path())
        .arg("doctor")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "1 / 1 references have source PDFs",
        ))
        .stdout(predicate::str::contains("source PDF missing").not());

    fs::write(t.path().join("second.pdf"), b"second source").unwrap();
    cmd(t.path())
        .args(["attach", "x", "second.pdf"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already has a source PDF"));
    assert_eq!(
        fs::read(t.path().join(".ref/source/x.pdf")).unwrap(),
        b"first source"
    );
    assert_eq!(
        fs::read(t.path().join("second.pdf")).unwrap(),
        b"second source"
    );
}
