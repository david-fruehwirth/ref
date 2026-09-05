mod support;

use r#ref::{
    model::{display_author, generated_key, CitationKey, Person, Reference, ReferenceType},
    repository::Repository,
};
use std::fs;

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

#[test]
fn public_formatting_and_key_generation_cover_author_shapes() {
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
    assert_eq!(generated_key(" Smith, Jr. ", Some(2024)), "smithjr2024");
    assert_eq!(generated_key("李", None), "reference");
}

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
