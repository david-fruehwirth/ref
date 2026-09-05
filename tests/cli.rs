use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;

fn cmd(dir: &std::path::Path) -> Command {
    let mut c = Command::cargo_bin("ref").unwrap();
    c.current_dir(dir);
    c
}

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
        fs::read(t.path().join(".ref/refs/x1/paper.pdf")).unwrap(),
        b"pdf"
    );
    cmd(t.path())
        .args(args)
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
}

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
    cmd(t.path()).arg("doctor").assert().success();
    cmd(t.path()).args(["doctor", "--strict"]).assert().code(1);
}
