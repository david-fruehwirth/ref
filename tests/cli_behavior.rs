mod support;

use predicates::prelude::*;
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    thread,
};

fn doi_server(status: &str, body: &str) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let status = status.to_owned();
    let body = body.to_owned();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0; 4096];
        let size = stream.read(&mut request).unwrap();
        assert!(String::from_utf8_lossy(&request[..size])
            .to_ascii_lowercase()
            .contains("accept: application/vnd.citationstyles.csl+json"));
        write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/vnd.citationstyles.csl+json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
    });
    (url, handle)
}

// Scenario: add from doi uses normal no pdf persistence and detects duplicates.
// Requirements: REQ-006, REQ-017
#[test]
fn add_from_doi_uses_normal_no_pdf_persistence_and_detects_duplicates() {
    let temp = tempfile::tempdir().unwrap();
    support::command(temp.path()).arg("init").assert().success();
    let fixture = r#"{"type":"article-journal","title":"Example Article","author":[{"given":"Jane","family":"Doe"}],"container-title":"Journal of Examples","DOI":"10.1234/EXAMPLE","issued":{"date-parts":[[2024,5,10]]}}"#;
    let (resolver, handle) = doi_server("200 OK", fixture);
    support::command(temp.path())
        .env("REF_DOI_RESOLVER", resolver)
        .args(["add", "--doi", "DOI: 10.1234/EXAMPLE"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added Doe2024ExampleArticle"))
        .stderr(predicate::str::contains("Retrieving metadata"));
    handle.join().unwrap();
    assert!(temp
        .path()
        .join(".ref/refs/Doe2024ExampleArticle/ref.yaml")
        .is_file());
    assert!(!temp
        .path()
        .join(".ref/source/Doe2024ExampleArticle.pdf")
        .exists());
    let yaml =
        fs::read_to_string(temp.path().join(".ref/refs/Doe2024ExampleArticle/ref.yaml")).unwrap();
    assert!(yaml.contains("doi: 10.1234/example"));
    support::command(temp.path())
        .args(["doctor", "--strict"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "Doe2024ExampleArticle: source PDF missing",
        ));
    support::command(temp.path())
        .args(["add", "--doi", "https://doi.org/10.1234/example"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "already stored as `Doe2024ExampleArticle`",
        ));
}

// Scenario: failed doi adds do not mutate the repository.
// Requirements: REQ-018, REQ-061
#[test]
fn failed_doi_adds_do_not_mutate_the_repository() {
    let temp = tempfile::tempdir().unwrap();
    support::command(temp.path()).arg("init").assert().success();
    support::command(temp.path())
        .args(["add", "--doi", "invalid"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid DOI"));
    let (resolver, handle) = doi_server("500 Internal Server Error", "");
    support::command(temp.path())
        .env("REF_DOI_RESOLVER", resolver)
        .args(["add", "--doi", "10.1234/failure"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("failed temporarily"));
    handle.join().unwrap();
    assert_eq!(
        fs::read_dir(temp.path().join(".ref/refs")).unwrap().count(),
        0
    );
}

// Scenario: add rejects a pdf and doi as competing sources.
// Requirement: REQ-014
#[test]
fn add_rejects_a_pdf_and_doi_as_competing_sources() {
    let temp = tempfile::tempdir().unwrap();
    support::command(temp.path()).arg("init").assert().success();
    fs::write(temp.path().join("paper.pdf"), b"%PDF").unwrap();
    support::command(temp.path())
        .args(["add", "paper.pdf", "--doi", "10.1234/example"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be used with"));
}

// Scenario: help and version expose stable command surface.
// Requirement: REQ-064
#[test]
fn help_and_version_expose_stable_command_surface() {
    let temp = tempfile::tempdir().unwrap();
    support::command(temp.path())
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "A Git-like, project-local reference manager",
        ))
        .stdout(predicate::str::contains("Add a bibliographic reference"))
        .stdout(predicate::str::contains("Search references"))
        .stdout(predicate::str::contains("Import references"))
        .stdout(predicate::str::contains("Export references"))
        .stdout(predicate::str::contains("Find and remove references"))
        .stdout(predicate::str::contains("Check repository integrity"));
    support::command(temp.path())
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

// Scenario: core subcommand help explains options and safety semantics.
// Requirement: REQ-064
#[test]
fn core_subcommand_help_explains_options_and_safety_semantics() {
    let temp = tempfile::tempdir().unwrap();

    support::command(temp.path())
        .args(["add", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--title"))
        .stdout(predicate::str::contains("--author"))
        .stdout(predicate::str::contains("--year"))
        .stdout(predicate::str::contains("--no-pdf"))
        .stdout(predicate::str::contains("Given names, Family name"));

    support::command(temp.path())
        .args(["clean", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--dry-run"))
        .stdout(predicate::str::contains("current directory downward"))
        .stdout(predicate::str::contains(
            "incomplete scan prevents deletion",
        ));

    support::command(temp.path())
        .args(["doctor", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--strict"))
        .stdout(predicate::str::contains("Treat warnings as failures"));

    support::command(temp.path())
        .args(["import", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("PDFs are not imported"))
        .stdout(predicate::str::contains("never overwritten"));
}

// Scenario: every documented command exposes scoped help without repository access.
// Requirement: REQ-064
#[test]
fn every_documented_command_exposes_scoped_help() {
    let temp = tempfile::tempdir().unwrap();
    for command in [
        "add", "attach", "list", "search", "show", "open", "edit", "rename", "remove", "clean",
        "import", "export", "doctor",
    ] {
        support::command(temp.path())
            .args([command, "--help"])
            .assert()
            .success()
            .stdout(predicate::str::contains("Usage:"))
            .stderr(predicate::str::is_empty());
    }
}

// Scenario: export stdout is an uncontaminated bibliography suitable for redirection.
// Requirements: REQ-050, REQ-066
#[test]
fn export_stdout_contains_only_bibliography_content() {
    let temp = tempfile::tempdir().unwrap();
    support::command(temp.path()).arg("init").assert().success();
    support::command(temp.path())
        .args(["add", "--no-pdf", "--key", "eeg2024", "--title", "EEG"])
        .assert()
        .success();

    let output = support::command(temp.path())
        .arg("export")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.starts_with("@article{eeg2024,"));
    assert!(!stdout.contains("Exported"));
}

// Scenario: rename keeps its result on stdout and its citation warning on stderr.
// Requirement: REQ-066
#[test]
fn rename_separates_result_from_warning() {
    let temp = tempfile::tempdir().unwrap();
    support::command(temp.path()).arg("init").assert().success();
    support::command(temp.path())
        .args(["add", "--no-pdf", "--key", "old", "--title", "EEG"])
        .assert()
        .success();
    support::command(temp.path())
        .args(["rename", "old", "new"])
        .assert()
        .success()
        .stdout(predicate::eq("Renamed old → new\n"))
        .stderr(predicate::str::contains("citations were not rewritten"));
}

// Scenario: cli wires init add list search show rename remove and export.
// Requirement: REQ-065
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
            "Jane, Smith",
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

// Scenario: list and search share readable, redirect-safe reference blocks.
// Requirements: REQ-021, REQ-022, REQ-066, REQ-073, REQ-074
#[test]
fn list_and_search_render_the_same_ordered_reference_blocks() {
    let temp = tempfile::tempdir().unwrap();
    let repo = r#ref::repository::Repository::init(temp.path()).unwrap();
    let mut first = support::sample("Études of Attention", "Núñez", Some(2024));
    first.authors.push(r#ref::model::Person {
        given: "Zoë".into(),
        family: "李".into(),
    });
    support::add(&repo, "Alpha2024", &first);
    let mut second = support::sample("Attention Without Dates", "", None);
    second.authors.clear();
    support::add(&repo, "Beta", &second);

    let expected = "Alpha2024\n    Title:   Études of Attention\n    Authors: Jane Núñez, Zoë 李\n    Year:    2024\n    Type:    article\n\nBeta\n    Title:   Attention Without Dates\n    Type:    article\n";
    for args in [vec!["list"], vec!["search", "attention"]] {
        support::command(temp.path())
            .args(args)
            .assert()
            .success()
            .stdout(predicate::eq(expected))
            .stdout(predicate::str::contains("\x1b[").not())
            .stdout(predicate::str::contains("KEY ").not())
            .stderr(predicate::str::is_empty());
    }
}

// Scenario: cli rejects missing pdf duplicate key and unknown reference without partial state.
// Requirements: REQ-006, REQ-018, REQ-023
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

// Scenario: doctor maps healthy warnings and errors to exit codes.
// Requirements: REQ-053, REQ-056
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
            "Jane, Smith",
            "--year",
            "2024",
        ])
        .assert()
        .success();
    support::command(healthy.path())
        .arg("doctor")
        .assert()
        .success()
        .stdout(predicate::str::contains("source PDF missing"));

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

// Scenario: doctor reports structural and metadata failures.
// Requirements: REQ-054, REQ-056
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

// Scenario: doctor distinguishes duplicate and malformed dois.
// Requirement: REQ-055
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
                "Jane, Smith",
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
            "Jane, Smith",
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

// Scenario: add accepts cli metadata attaches pdf and generates key.
// Requirements: REQ-012, REQ-015
#[test]
fn add_accepts_cli_metadata_attaches_pdf_and_generates_key() {
    let temp = tempfile::tempdir().unwrap();
    support::command(temp.path()).arg("init").assert().success();
    fs::write(temp.path().join("paper.pdf"), b"%PDF-1.4\n").unwrap();
    support::command(temp.path())
        .args([
            "add",
            "paper.pdf",
            "--title",
            "Example Paper",
            "--author",
            "John,Doe",
            "--author",
            " Sam , Altman ",
            "--year",
            "2024",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added Doe2024ExamplePaper"));
    let stored = r#ref::repository::Repository::discover(temp.path())
        .unwrap()
        .load_reference(&r#ref::model::CitationKey::new("Doe2024ExamplePaper").unwrap())
        .unwrap();
    assert_eq!(stored.metadata.title, "Example Paper");
    assert_eq!(stored.metadata.year, Some(2024));
    assert_eq!(
        stored.metadata.authors,
        vec![
            r#ref::model::Person {
                given: "John".into(),
                family: "Doe".into()
            },
            r#ref::model::Person {
                given: "Sam".into(),
                family: "Altman".into()
            },
        ]
    );
    assert!(stored.has_pdf);
    support::command(temp.path())
        .arg("doctor")
        .assert()
        .success()
        .stdout(predicate::str::contains("0 warnings, 0 errors"));
}

// Scenario: add without pdf uses metadata and rejects bad inputs and conflicts.
// Requirements: REQ-006, REQ-014, REQ-016, REQ-018
#[test]
fn add_without_pdf_uses_metadata_and_rejects_bad_inputs_and_conflicts() {
    let temp = tempfile::tempdir().unwrap();
    support::command(temp.path()).arg("init").assert().success();
    support::command(temp.path())
        .args([
            "add",
            "--no-pdf",
            "--title",
            "Example",
            "--author",
            " John , Doe ",
            "--year",
            "2024",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added Doe2024Example"));
    assert!(!temp.path().join(".ref/source/Doe2024Example.pdf").exists());

    for author in ["John Doe", "John,", ",Doe"] {
        support::command(temp.path())
            .args([
                "add", "--no-pdf", "--title", "Bad", "--author", author, "--year", "2024",
            ])
            .assert()
            .failure()
            .stderr(predicate::str::contains("Given names, Family name"));
    }
    support::command(temp.path())
        .args(["add", "--no-pdf", "--title", "Bad year", "--year", "999"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not plausible"));
    fs::write(temp.path().join("other.pdf"), b"%PDF").unwrap();
    support::command(temp.path())
        .args(["add", "other.pdf", "--no-pdf", "--title", "Bad"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be used with"));
    assert!(!temp.path().join(".ref/refs/bad").exists());
}

// Scenario: generated add keys handle normalization collisions and explicit override.
// Requirements: REQ-012, REQ-013
#[test]
fn generated_add_keys_handle_normalization_collisions_and_explicit_override() {
    let temp = tempfile::tempdir().unwrap();
    support::command(temp.path()).arg("init").assert().success();
    let args = [
        "add",
        "--no-pdf",
        "--title",
        "Molecular Interaction Models",
        "--author",
        "Johannes, van der Waals",
        "--year",
        "2024",
    ];
    support::command(temp.path())
        .args(args)
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Added VanDerWaals2024MolecularInteraction",
        ));
    support::command(temp.path())
        .args(args)
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Added VanDerWaals2024MolecularInteractionA",
        ));
    support::command(temp.path())
        .args(args)
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Added VanDerWaals2024MolecularInteractionB",
        ));

    support::command(temp.path())
        .args([
            "add",
            "--no-pdf",
            "--key",
            "myKey",
            "--title",
            "Example Paper",
            "--year",
            "2024",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added myKey"));
    support::command(temp.path())
        .args([
            "add",
            "--no-pdf",
            "--title",
            "Example Paper",
            "--author",
            "Jane, Smith",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("provide `--year`"))
        .stderr(predicate::str::contains("`--key`"));
}
