mod support;

use r#ref::{
    doctor::{self, DiagnosticSeverity, DoctorDiagnostic},
    model::{display_author, generated_key, CitationKey, Person, Reference, ReferenceType},
    repository::Repository,
};
use std::fs;

// Scenario: citation keys accept safe components and reject traversal.
// Requirement: REQ-005
#[test]
fn citation_keys_accept_safe_components_and_reject_traversal() {
    for valid in [
        "rocchio1971",
        "vaswani2017",
        "smith2024attention",
        "foo-bar",
        "foo_bar",
        "foo.bar",
        "AReference2024",
    ] {
        assert!(
            CitationKey::new(valid).is_ok(),
            "expected {valid:?} to be valid"
        );
    }
    for invalid in [
        "", " ", "foo bar", "/foo", "foo/bar", "foo\\bar", ".", "..", "../x",
    ] {
        assert!(
            CitationKey::new(invalid).is_err(),
            "expected {invalid:?} to be invalid"
        );
    }
}

// Scenario: public formatting covers author shapes.
// Requirement: REQ-010
#[test]
fn public_formatting_covers_author_shapes() {
    let people = [
        Person {
            given: "A".into(),
            family: "Vaswani".into(),
        },
        Person {
            given: "N".into(),
            family: "Shazeer".into(),
        },
        Person {
            given: "N".into(),
            family: "Parmar".into(),
        },
    ];
    assert_eq!(display_author(&[]), "-");
    assert_eq!(display_author(&people[..1]), "Vaswani");
    assert_eq!(display_author(&people[..2]), "Vaswani & Shazeer");
    assert_eq!(display_author(&people), "Vaswani et al.");
}

// Scenario: generated keys normalize author year and title.
// Requirement: REQ-012
#[test]
fn generated_keys_normalize_author_year_and_title() {
    let cases = [
        (
            "Rocchio",
            "Relevance Feedback in Information Retrieval",
            "Rocchio1971RelevanceFeedback",
        ),
        (
            "van der Waals",
            "Molecular Interaction Models",
            "VanDerWaals1971MolecularInteraction",
        ),
        ("Müller", "Neural Signal Analysis", "Muller1971NeuralSignal"),
        (
            "García Márquez",
            "Computational Language Models",
            "GarciaMarquez1971ComputationalLanguage",
        ),
        (
            "O'Connor",
            "EEG Attention Measurement",
            "OConnor1971EEGAttention",
        ),
        (
            "Smith-Jones",
            "attention mechanisms in transformers",
            "SmithJones1971AttentionMechanisms",
        ),
        (
            "Smith",
            "Attention mechanisms in transformers",
            "Smith1971AttentionMechanisms",
        ),
        (
            "Smith",
            "Using EEG for Attention Detection",
            "Smith1971UsingEEG",
        ),
        ("Smith", "Attention", "Smith1971Attention"),
    ];
    for (family, title, expected) in cases {
        let reference = support::sample(title, family, Some(1971));
        let key = generated_key(&reference).unwrap();
        assert_eq!(key.as_str(), expected);
        assert!(CitationKey::new(key.as_str()).is_ok());
    }
}

// Scenario: generated keys require author and year but explicit keys remain valid.
// Requirement: REQ-013
#[test]
fn generated_keys_require_author_and_year_but_explicit_keys_remain_valid() {
    let mut reference = support::sample("Example Paper", "Smith", None);
    assert!(generated_key(&reference)
        .unwrap_err()
        .to_string()
        .contains("publication year"));
    reference.year = Some(2024);
    reference.authors.clear();
    assert!(generated_key(&reference)
        .unwrap_err()
        .to_string()
        .contains("without an author"));
    assert!(CitationKey::new("custom").is_ok());
}

// Scenario: metadata round trips unicode optional fields and types.
// Requirements: REQ-008, REQ-009
#[test]
fn metadata_round_trips_unicode_optional_fields_and_types() {
    for entry_type in [
        ReferenceType::Article,
        ReferenceType::Book,
        ReferenceType::Inproceedings,
        ReferenceType::Thesis,
        ReferenceType::Report,
        ReferenceType::Online,
    ] {
        let mut reference = support::sample("Gödel, Escher, Bach", "Müller", Some(1979));
        reference.entry_type = entry_type;
        reference.tags = vec!["logic".into(), "François".into()];
        reference.notes = Some("naïve set theory".into());
        let yaml = serde_yaml::to_string(&reference).unwrap();
        let decoded: Reference = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(decoded, reference);
    }
    assert!(serde_yaml::from_str::<Reference>("type: article\nauthors: []\n").is_err());
    assert!(serde_yaml::from_str::<Reference>("type: unknown\ntitle: X\nauthors: []\n").is_err());
}

// Scenario: initialization discovery and nearest nested repository are filesystem contracts.
// Requirements: REQ-001, REQ-002, REQ-003
#[test]
fn initialization_discovery_and_nearest_nested_repository_are_filesystem_contracts() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("keep.txt"), "untouched").unwrap();
    let outer = Repository::init(temp.path()).unwrap();
    assert!(outer.root().join("config.yaml").is_file());
    assert!(outer.references_dir().is_dir());
    assert_eq!(
        fs::read_to_string(temp.path().join("keep.txt")).unwrap(),
        "untouched"
    );
    assert!(Repository::init(temp.path()).is_err());

    let nested = temp.path().join("chapters/recommendation/section");
    fs::create_dir_all(&nested).unwrap();
    assert_eq!(Repository::discover(&nested).unwrap().root(), outer.root());
    let inner_root = temp.path().join("chapters");
    let inner = Repository::init(&inner_root).unwrap();
    assert_eq!(Repository::discover(&nested).unwrap().root(), inner.root());
}

// Scenario: repository loads manual edits and derives identity from directory.
// Requirement: REQ-004
#[test]
fn repository_loads_manual_edits_and_derives_identity_from_directory() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path()).unwrap();
    for key in ["zeta2024", "alpha2023"] {
        let directory = repo.references_dir().join(key);
        fs::create_dir(&directory).unwrap();
        fs::write(
            directory.join("ref.yaml"),
            serde_yaml::to_string(&support::sample(key, "Smith", Some(2024))).unwrap(),
        )
        .unwrap();
    }
    fs::write(
        repo.reference_path(&CitationKey::new("zeta2024").unwrap())
            .join("paper.pdf"),
        b"%PDF tiny",
    )
    .unwrap();
    let loaded = repo.load_all().unwrap();
    assert_eq!(
        loaded.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(),
        ["alpha2023", "zeta2024"]
    );
    assert!(!loaded[0].has_pdf);
    assert!(loaded[1].has_pdf);
}

// Scenario: doctor structures source proof diagnostics and counts.
// Requirements: REQ-053, REQ-056
#[test]
fn doctor_structures_source_proof_diagnostics_and_counts() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path()).unwrap();
    for key in ["RefA", "RefB", "RefC", "RefD"] {
        support::add(&repo, key, &support::sample(key, "Smith", Some(2024)));
    }
    for key in ["RefA", "RefC"] {
        fs::write(
            repo.reference_path(&CitationKey::new(key).unwrap())
                .join("paper.pdf"),
            b"deterministic source bytes",
        )
        .unwrap();
    }
    fs::write(
        repo.reference_path(&CitationKey::new("RefB").unwrap())
            .join("source.pdf"),
        b"wrong name",
    )
    .unwrap();

    let report = doctor::inspect(&repo).unwrap();
    assert_eq!(report.references_total, 4);
    assert_eq!(report.references_with_source_pdf, 2);
    assert_eq!(report.references_without_source_pdf, 2);
    let missing = report
        .diagnostics
        .iter()
        .filter_map(|diagnostic| match diagnostic {
            DoctorDiagnostic::MissingSourcePdf { key } => Some(key.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(missing, ["RefB", "RefD"]);
    assert!(report.diagnostics.iter().all(|diagnostic| {
        !matches!(diagnostic, DoctorDiagnostic::MissingSourcePdf { .. })
            || diagnostic.severity() == DiagnosticSeverity::Warning
    }));
}

// Scenario: doctor rejects empty and non file source proof.
// Requirement: REQ-053
#[test]
fn doctor_rejects_empty_and_non_file_source_proof() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path()).unwrap();
    for key in ["empty", "directory"] {
        support::add(&repo, key, &support::sample(key, "Smith", Some(2024)));
    }
    fs::write(
        repo.reference_path(&CitationKey::new("empty").unwrap())
            .join("paper.pdf"),
        [],
    )
    .unwrap();
    fs::create_dir(
        repo.reference_path(&CitationKey::new("directory").unwrap())
            .join("paper.pdf"),
    )
    .unwrap();
    let report = doctor::inspect(&repo).unwrap();
    assert_eq!(report.references_with_source_pdf, 0);
    assert_eq!(report.error_count(), 2);
    assert!(report.diagnostics.iter().all(|diagnostic| {
        !matches!(diagnostic, DoctorDiagnostic::InvalidSourcePdf { .. })
            || diagnostic.severity() == DiagnosticSeverity::Error
    }));
}

// Scenario: add rename and remove preserve data and reject unsafe mutations.
// Requirements: REQ-006, REQ-028, REQ-029, REQ-030
#[test]
fn add_rename_and_remove_preserve_data_and_reject_unsafe_mutations() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path()).unwrap();
    let source = temp.path().join("source.pdf");
    fs::write(&source, b"%PDF deterministic").unwrap();
    let old = CitationKey::new("old2024").unwrap();
    let new = CitationKey::new("new2024").unwrap();
    let metadata = support::sample("Safe mutation", "Smith", Some(2024));
    repo.add(&old, &metadata, Some(&source)).unwrap();
    assert_eq!(fs::read(&source).unwrap(), b"%PDF deterministic");
    assert!(repo
        .add(&old, &support::sample("overwrite", "X", None), None)
        .is_err());
    repo.rename(&old, &new).unwrap();
    assert!(!repo.contains(&old));
    let loaded = repo.load_reference(&new).unwrap();
    assert_eq!(loaded.metadata, metadata);
    assert_eq!(
        fs::read(loaded.path.join("paper.pdf")).unwrap(),
        b"%PDF deterministic"
    );
    repo.remove(&new).unwrap();
    assert!(!repo.contains(&new));
    assert!(repo.remove(&new).is_err());
}

// Scenario: failed pdf copy leaves no final reference.
// Requirements: REQ-007, REQ-018
#[test]
fn failed_pdf_copy_leaves_no_final_reference() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path()).unwrap();
    let key = CitationKey::new("missing2024").unwrap();
    assert!(repo
        .add(
            &key,
            &support::sample("Missing", "Smith", None),
            Some(&temp.path().join("absent.pdf"))
        )
        .is_err());
    assert!(!repo.contains(&key));
}
