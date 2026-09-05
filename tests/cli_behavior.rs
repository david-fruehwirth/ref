mod support;

use predicates::prelude::*;
use std::fs;

#[test]
fn help_and_version_expose_stable_command_surface() {
    let temp = tempfile::tempdir().unwrap();
    support::command(temp.path())
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("search"))
        .stdout(predicate::str::contains("doctor"));
    support::command(temp.path())
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn cli_wires_init_add_list_search_show_rename_remove_and_export() {
    let temp = tempfile::tempdir().unwrap();
    support::command(temp.path()).arg("init").assert().success();
    support::command(temp.path())
        .args([
            "add",
            "--no-pdf",
            "--key",
            "smith2024",
            "--title",
            "Relevance Feedback",
            "--author",
            "Jane|Smith",
            "--year",
            "2024",
            "--tags",
            "retrieval,classic",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added smith2024"));

    support::command(temp.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("smith2024"))
        .stdout(predicate::str::contains("Relevance Feedback"));
    for query in ["SMITH", "relevance", "2024", "classic", "smith2024"] {
        support::command(temp.path())
            .args(["search", query])
            .assert()
            .success()
            .stdout(predicate::str::contains("smith2024"));
    }
    support::command(temp.path())
        .args(["search", "smith", "--author"])
        .assert()
        .success()
        .stdout(predicate::str::contains("smith2024"));
    support::command(temp.path())
        .args(["search", "smith", "--title"])
        .assert()
        .success()
        .stdout(predicate::str::contains("smith2024").not());
    support::command(temp.path())
        .args(["show", "smith2024"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Jane Smith"))
        .stdout(predicate::str::contains("PDF:       no"));
    support::command(temp.path())
        .arg("export")
        .assert()
        .success()
        .stdout(predicate::str::contains("@article{smith2024"));
    support::command(temp.path())
        .args(["rename", "smith2024", "smith2025"])
        .assert()
        .success();
    support::command(temp.path())
        .args(["remove", "smith2025"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("confirmation required"));
    support::command(temp.path())
        .args(["remove", "smith2025", "--yes"])
        .assert()
        .success();
    assert!(!temp.path().join(".ref/refs/smith2025").exists());
}

#[test]
fn cli_rejects_missing_pdf_duplicate_key_and_unknown_reference_without_partial_state() {
    let temp = tempfile::tempdir().unwrap();
    support::command(temp.path()).arg("init").assert().success();
    support::command(temp.path())
        .args(["add", "missing.pdf", "--key", "bad", "--title", "Bad"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("does not exist"));
    assert!(!temp.path().join(".ref/refs/bad").exists());
    support::command(temp.path())
        .args(["add", "--no-pdf", "--key", "good", "--title", "Good"])
        .assert()
        .success()
        .stderr(predicate::str::contains("warning"));
    support::command(temp.path())
        .args(["add", "--no-pdf", "--key", "good", "--title", "Overwrite"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
    support::command(temp.path())
        .args(["show", "unknown"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("does not exist"));
}

#[test]
fn doctor_maps_healthy_warnings_and_errors_to_exit_codes() {
    let healthy = tempfile::tempdir().unwrap();
    support::command(healthy.path())
        .arg("init")
        .assert()
        .success();
    support::command(healthy.path())
        .args([
            "add",
            "--no-pdf",
            "--key",
            "healthy2024",
            "--title",
            "Healthy",
            "--author",
            "Jane|Smith",
            "--year",
            "2024",
        ])
        .assert()
        .success();
    support::command(healthy.path())
        .arg("doctor")
        .assert()
        .success()
        .stdout(predicate::str::contains("0 warnings, 0 errors"));

    let warning = tempfile::tempdir().unwrap();
    support::command(warning.path())
        .arg("init")
        .assert()
        .success();
    support::command(warning.path())
        .args(["add", "--no-pdf", "--key", "warning", "--title", "Warning"])
        .assert()
        .success();
    support::command(warning.path())
        .arg("doctor")
        .assert()
        .success()
        .stdout(predicate::str::contains("Warnings:"));
    support::command(warning.path())
        .args(["doctor", "--strict"])
        .assert()
        .code(1);

    let broken = tempfile::tempdir().unwrap();
    support::command(broken.path())
        .arg("init")
        .assert()
        .success();
    fs::create_dir(broken.path().join(".ref/refs/bad key")).unwrap();
    support::command(broken.path())
        .arg("doctor")
        .assert()
        .code(1)
        .stdout(predicate::str::contains("invalid citation key"));
}

#[test]
fn doctor_reports_structural_and_metadata_failures() {
    for mutation in ["config", "refs"] {
        let temp = tempfile::tempdir().unwrap();
        support::command(temp.path()).arg("init").assert().success();
        if mutation == "config" {
            fs::remove_file(temp.path().join(".ref/config.yaml")).unwrap();
        } else {
            fs::remove_dir(temp.path().join(".ref/refs")).unwrap();
        }
        support::command(temp.path())
            .arg("doctor")
            .assert()
            .failure();
    }
    let temp = tempfile::tempdir().unwrap();
    support::command(temp.path()).arg("init").assert().success();
    let refs = temp.path().join(".ref/refs");
    fs::create_dir(refs.join("missing")).unwrap();
    fs::create_dir(refs.join("malformed")).unwrap();
    fs::write(refs.join("malformed/ref.yaml"), "not: [yaml").unwrap();
    support::command(temp.path())
        .arg("doctor")
        .assert()
        .code(1)
        .stdout(predicate::str::contains("ref.yaml missing"))
        .stdout(predicate::str::contains("invalid metadata"));
}

#[test]
fn doctor_distinguishes_duplicate_and_malformed_dois() {
    let warning = tempfile::tempdir().unwrap();
    support::command(warning.path())
        .arg("init")
        .assert()
        .success();
    for key in ["one", "two"] {
        support::command(warning.path())
            .args([
                "add",
                "--no-pdf",
                "--key",
                key,
                "--title",
                key,
                "--author",
                "Jane|Smith",
                "--year",
                "2024",
                "--doi",
                "10.1000/same",
            ])
            .assert()
            .success();
    }
    support::command(warning.path())
        .arg("doctor")
        .assert()
        .success()
        .stdout(predicate::str::contains("duplicate DOI"));
    support::command(warning.path())
        .args(["doctor", "--strict"])
        .assert()
        .code(1);

    let malformed = tempfile::tempdir().unwrap();
    support::command(malformed.path())
        .arg("init")
        .assert()
        .success();
    support::command(malformed.path())
        .args([
            "add",
            "--no-pdf",
            "--key",
            "bad-doi",
            "--title",
            "Bad DOI",
            "--author",
            "Jane|Smith",
            "--year",
            "2024",
            "--doi",
            "not-a-doi",
        ])
        .assert()
        .success();
    support::command(malformed.path())
        .arg("doctor")
        .assert()
        .code(1)
        .stdout(predicate::str::contains("malformed DOI"));
}
