use r#ref::{
    export::biblatex,
    import::{import_bibliography, parse_bibliography},
    model::{
        AccessDate, Author, CitationKey, Organization, PublicationDate, Reference, ReferenceType,
    },
    repository::Repository,
};

// Scenario: Publication dates accept every supported precision and reject impossible dates.
// Requirements: REQ-097
#[test]
fn publication_date_validation() {
    for value in ["2024", "2024-10", "2024-10-03", "2024-02-29"] {
        assert_eq!(PublicationDate::new(value).unwrap().as_str(), value);
    }
    for value in ["2024-13", "2024-02-30", "24-10-03", "foo"] {
        assert!(PublicationDate::new(value).is_err(), "{value}");
    }
}

// Scenario: A complete web access date is required and survives YAML serialization.
// Requirements: REQ-099, REQ-111, REQ-112
#[test]
fn web_access_date_and_yaml_round_trip() {
    assert!(AccessDate::new("2026-09-19").is_ok());
    for value in ["2026", "2026-09", "2026-02-30"] {
        assert!(AccessDate::new(value).is_err());
    }
    let yaml = "type: online\ntitle: Example\nauthors:\n  - organization: Example Org\ndate: 2024-10-03\nyear: 2024\nurl: https://example.org\nurldate: 2026-09-19\n";
    let reference: Reference = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(reference.date.as_ref().unwrap().as_str(), "2024-10-03");
    assert_eq!(reference.urldate.as_ref().unwrap().as_str(), "2026-09-19");
    assert!(serde_yaml::from_str::<Reference>(&format!("{yaml}unknown: value\n")).is_err());
    assert!(serde_yaml::from_str::<Reference>(
        "type: online\ntitle: X\nauthors:\n - organization: Corp\n   family: Person\n"
    )
    .is_err());
}

// Scenario: Online fields, a corporate author, and a note are emitted deterministically.
// Requirements: REQ-096, REQ-098, REQ-100, REQ-101, REQ-102, REQ-110, REQ-113
#[test]
fn exports_complete_online_reference() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path()).unwrap();
    let reference = Reference {
        entry_type: ReferenceType::Online,
        title: "Example".into(),
        authors: vec![Author::Organization(Organization {
            organization: "YouTube".into(),
        })],
        year: Some(2024),
        date: Some(PublicationDate::new("2024-10-03").unwrap()),
        container_title: None,
        publisher: None,
        volume: None,
        issue: None,
        pages: None,
        doi: None,
        url: Some("https://example.org".into()),
        urldate: Some(AccessDate::new("2026-09-19").unwrap()),
        tags: vec![],
        notes: Some("Useful".into()),
    };
    repo.add(&CitationKey::new("web").unwrap(), &reference, None)
        .unwrap();
    let out = biblatex(&repo.load_all().unwrap());
    assert!(out.contains("@online{web,"));
    assert!(out.contains("author  = {{YouTube}},"));
    assert!(out.contains("date    = {2024-10-03},"));
    assert!(!out.contains("year    ="));
    assert!(out.contains("urldate = {2026-09-19},"));
    assert!(out.contains("note    = {Useful},"));
}

// Scenario: BibLaTeX web metadata survives import, YAML persistence, and export, including aliases.
// Requirements: REQ-103, REQ-105, REQ-106, REQ-107, REQ-108, REQ-109
#[test]
fn imports_and_round_trips_web_metadata() {
    for kind in ["online", "electronic", "www"] {
        let temp = tempfile::tempdir().unwrap();
        let repo = Repository::init(temp.path()).unwrap();
        let bib=format!("@{kind}{{web, author={{{{Example Group}} and Smith, Jane}}, title={{Example}}, date={{2024-10-03}}, year={{2024}}, url={{https://example.org}}, urldate={{2026-09-19}}}}");
        let result = import_bibliography(&repo, parse_bibliography(&bib).unwrap());
        assert!(result.is_complete());
        let stored = repo
            .load_reference(&CitationKey::new("web").unwrap())
            .unwrap();
        assert!(
            matches!(&stored.metadata.authors[0], Author::Organization(o) if o.organization=="Example Group")
        );
        assert_eq!(stored.metadata.authors[1].display_name(), "Smith");
        let out = biblatex(&[stored]);
        assert!(out.contains("date    = {2024-10-03},"));
        assert!(out.contains("urldate = {2026-09-19},"));
    }
}
