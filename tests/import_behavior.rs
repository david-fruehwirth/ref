mod support;

use predicates::prelude::*;
use r#ref::{
    import::{import_bibliography, parse_bibliography, DiagnosticKind},
    model::{CitationKey, ReferenceType},
    repository::Repository,
};
use std::fs;

fn initialized() -> (tempfile::TempDir, Repository) {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path()).unwrap();
    (temp, repo)
}

// Scenario: imports common fields authors macros unicode and no pdf.
// Requirements: REQ-037, REQ-038, REQ-049
#[test]
fn imports_common_fields_authors_macros_unicode_and_no_pdf() {
    let (temp, repo) = initialized();
    let bib = r#"
@string{journalName = "Example Journal"}
@article{smith2024,
  author = {Smith, Jane and John Doe},
  title = {Using {EEG}: Müller, François, and R&D},
  journal = journalName,
  journaltitle = {Preferred Journal},
  year = {2024}, volume = {XII}, number = {3}, pages = {10--20},
  doi = {https://doi.org/10.1234/example}, url = {https://example.test},
  keywords = {eeg, attention, neuroscience}, note = {A note},
  file = {/some/path/paper.pdf}
}"#;
    let result = import_bibliography(&repo, parse_bibliography(bib).unwrap());
    assert!(result.is_complete());
    assert_eq!(result.imported, 1);
    let stored = repo
        .load_reference(&CitationKey::new("smith2024").unwrap())
        .unwrap();
    assert_eq!(
        stored.metadata.title,
        "Using EEG: Müller, François, and R&D"
    );
    assert_eq!(stored.metadata.authors[0].family, "Smith");
    assert_eq!(stored.metadata.authors[0].given, "Jane");
    assert_eq!(stored.metadata.authors[1].family, "Doe");
    assert_eq!(stored.metadata.authors[1].given, "John");
    assert_eq!(
        stored.metadata.container_title.as_deref(),
        Some("Preferred Journal")
    );
    assert_eq!(stored.metadata.pages.as_deref(), Some("10-20"));
    assert_eq!(stored.metadata.doi.as_deref(), Some("10.1234/example"));
    assert_eq!(stored.metadata.tags, ["eeg", "attention", "neuroscience"]);
    assert!(!stored.has_pdf);
    assert!(!temp.path().join(".ref/refs/smith2024/paper.pdf").exists());
}

// Scenario: maps entry types and date.
// Requirements: REQ-039, REQ-044, REQ-045
#[test]
fn maps_entry_types_and_date() {
    let (_temp, repo) = initialized();
    let cases = [
        ("article", ReferenceType::Article),
        ("book", ReferenceType::Book),
        ("inproceedings", ReferenceType::Inproceedings),
        ("phdthesis", ReferenceType::Thesis),
        ("mastersthesis", ReferenceType::Thesis),
        ("techreport", ReferenceType::Report),
        ("online", ReferenceType::Online),
        ("misc", ReferenceType::Misc),
    ];
    let bib = cases
        .iter()
        .enumerate()
        .map(|(i, (kind, _))| {
            format!(
                "@{kind}{{key{i}, author={{Smith, Jane}}, title={{Title}}, date={{2024-05-17}}}}"
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let result = import_bibliography(&repo, parse_bibliography(&bib).unwrap());
    assert_eq!(result.imported, cases.len());
    for (i, (_, expected)) in cases.iter().enumerate() {
        let stored = repo
            .load_reference(&CitationKey::new(format!("key{i}")).unwrap())
            .unwrap();
        assert_eq!(&stored.metadata.entry_type, expected);
        assert_eq!(stored.metadata.year, Some(2024));
    }
}

// Scenario: imports all canonical types aliases and mixed case types.
// Requirements: REQ-044, REQ-045, REQ-046, REQ-048
#[test]
fn imports_all_canonical_types_aliases_and_mixed_case_types() {
    let (_temp, repo) = initialized();
    let cases = [
        ("article", ReferenceType::Article),
        ("book", ReferenceType::Book),
        ("mvbook", ReferenceType::Mvbook),
        ("inbook", ReferenceType::Inbook),
        ("bookinbook", ReferenceType::Bookinbook),
        ("suppbook", ReferenceType::Suppbook),
        ("booklet", ReferenceType::Booklet),
        ("collection", ReferenceType::Collection),
        ("mvcollection", ReferenceType::Mvcollection),
        ("incollection", ReferenceType::Incollection),
        ("suppcollection", ReferenceType::Suppcollection),
        ("dataset", ReferenceType::Dataset),
        ("manual", ReferenceType::Manual),
        ("misc", ReferenceType::Misc),
        ("online", ReferenceType::Online),
        ("patent", ReferenceType::Patent),
        ("periodical", ReferenceType::Periodical),
        ("suppperiodical", ReferenceType::Suppperiodical),
        ("proceedings", ReferenceType::Proceedings),
        ("mvproceedings", ReferenceType::Mvproceedings),
        ("inproceedings", ReferenceType::Inproceedings),
        ("reference", ReferenceType::Reference),
        ("mvreference", ReferenceType::Mvreference),
        ("inreference", ReferenceType::Inreference),
        ("report", ReferenceType::Report),
        ("set", ReferenceType::Set),
        ("software", ReferenceType::Software),
        ("thesis", ReferenceType::Thesis),
        ("unpublished", ReferenceType::Unpublished),
        ("xdata", ReferenceType::Xdata),
        ("conference", ReferenceType::Inproceedings),
        ("electronic", ReferenceType::Online),
        ("www", ReferenceType::Online),
        ("mastersthesis", ReferenceType::Thesis),
        ("phdthesis", ReferenceType::Thesis),
        ("techreport", ReferenceType::Report),
        ("Article", ReferenceType::Article),
        ("PhDThesis", ReferenceType::Thesis),
        ("UNPUBLISHED", ReferenceType::Unpublished),
        ("TECHREPORT", ReferenceType::Report),
    ];
    let bib = cases.iter().enumerate().map(|(i, (kind, _))| format!(
        "@{kind}{{type{i}, author={{Doe, Jane}}, title={{Work in Progress}}, note={{Unpublished manuscript}}}}"
    )).collect::<Vec<_>>().join("\n");

    let entries = parse_bibliography(&format!("@COMMENT{{ignored entry}}\n{bib}")).unwrap();
    assert_eq!(entries.len(), cases.len());
    let result = import_bibliography(&repo, entries);
    assert_eq!(result.imported, cases.len());
    assert!(result.warnings.is_empty());
    for (i, (_, expected)) in cases.iter().enumerate() {
        let stored = repo
            .load_reference(&CitationKey::new(format!("type{i}")).unwrap())
            .unwrap();
        assert_eq!(&stored.metadata.entry_type, expected);
    }
}

// Scenario: partial failures conflicts and unknown types are structured.
// Requirements: REQ-041, REQ-042, REQ-047
#[test]
fn partial_failures_conflicts_and_unknown_types_are_structured() {
    let (_temp, repo) = initialized();
    let first =
        parse_bibliography("@article{existing, author={Old, One}, title={Original}, year={2020}}")
            .unwrap();
    import_bibliography(&repo, first);
    let bib = r#"
@article{good-a, author={Smith, Jane}, title={A}}
@article{bad key, author={Smith, Jane}, title={Bad}}
@article{missing, author={Smith, Jane}}
@article{existing, author={New, Person}, title={Overwrite}}
@customa{good-c, author={{World Health Organization}}, title={C}}
@article{good-a, author={Other, Person}, title={Duplicate}}
"#;
    let result = import_bibliography(&repo, parse_bibliography(bib).unwrap());
    assert_eq!(result.total, 6);
    assert_eq!(result.imported, 2);
    assert_eq!(result.failed.len(), 2);
    assert_eq!(result.skipped.len(), 2);
    assert_eq!(result.warnings.len(), 1);
    assert_eq!(
        result.warnings[0].message,
        "unsupported type `customa`; imported as `misc`"
    );
    assert!(result
        .failed
        .iter()
        .any(|d| d.kind == DiagnosticKind::InvalidCitationKey));
    assert_eq!(
        repo.load_reference(&CitationKey::new("existing").unwrap())
            .unwrap()
            .metadata
            .title,
        "Original"
    );
    assert!(repo.contains(&CitationKey::new("good-c").unwrap()));
}

// Scenario: malformed file is rejected before mutation.
// Requirement: REQ-040
#[test]
fn malformed_file_is_rejected_before_mutation() {
    let (_temp, repo) = initialized();
    let malformed = "@article{first, title={Fine}}\n@article{broken, title={oops}";
    let error = parse_bibliography(malformed).unwrap_err();
    assert!(error.to_string().contains("line 2"));
    assert!(repo.load_all().unwrap().is_empty());
}

// Scenario: cli imports exports and reports partial status.
// Requirements: REQ-041, REQ-043
#[test]
fn cli_imports_exports_and_reports_partial_status() {
    let temp = tempfile::tempdir().unwrap();
    support::command(temp.path()).arg("init").assert().success();
    fs::write(
        temp.path().join("references.bib"),
        "@article{smith2024, author={Smith, Jane}, title={Example}, journal={Journal}, year={2024}, pages={10--20}}",
    )
    .unwrap();
    support::command(temp.path())
        .args(["import", "references.bib"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Imported 1 references"));
    support::command(temp.path())
        .arg("export")
        .assert()
        .success()
        .stdout(predicate::str::contains("@article{smith2024"))
        .stdout(predicate::str::contains("Journal"))
        .stdout(predicate::str::contains("10--20"));
    support::command(temp.path())
        .args(["show", "smith2024"])
        .assert()
        .success()
        .stdout(predicate::str::contains("PDF:       no"));
    support::command(temp.path())
        .args(["search", "Example"])
        .assert()
        .success()
        .stdout(predicate::str::contains("smith2024"));
    support::command(temp.path())
        .arg("doctor")
        .assert()
        .success()
        .stdout(predicate::str::contains("smith2024: source PDF missing"));

    fs::write(
        temp.path().join("partial.bib"),
        "@article{next, title={Next}}\n@article{broken, author={Smith, Jane}}",
    )
    .unwrap();
    support::command(temp.path())
        .args(["import", "partial.bib"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("Imported: 1"))
        .stderr(predicate::str::contains("failed to import `broken`"));
    assert!(temp.path().join("partial.bib").is_file());
}
