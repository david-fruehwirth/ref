mod support;

use r#ref::{
    export::biblatex,
    model::{CitationKey, Person, ReferenceType},
    repository::StoredReference,
};
use std::path::PathBuf;

fn stored(key: &str) -> StoredReference {
    let mut metadata = support::sample(
        "A & B % C $ D # E _ {F} ~ G ^ H \\ I — Gödel",
        "Müller",
        Some(2024),
    );
    metadata.authors.push(Person {
        given: "François".into(),
        family: "Curie".into(),
    });
    metadata.container_title = Some("Proceedings".into());
    metadata.publisher = Some("Press".into());
    metadata.volume = Some("4".into());
    metadata.issue = Some("2".into());
    metadata.pages = Some("313-323".into());
    metadata.doi = Some("10.1000/example".into());
    metadata.url = Some("https://example.test/a_b".into());
    StoredReference {
        key: CitationKey::new(key).unwrap(),
        metadata,
        path: PathBuf::new(),
        has_pdf: false,
        pdf_filename: None,
        pdf_path: None,
        added_at: None,
    }
}

// Scenario: biblatex is deterministic sorted and escapes public output.
// Requirements: REQ-050, REQ-051
#[test]
fn biblatex_is_deterministic_sorted_and_escapes_public_output() {
    let mut z = stored("zeta2024");
    z.metadata.entry_type = ReferenceType::Inproceedings;
    let a = stored("alpha2024");
    let first = biblatex(&[z.clone(), a.clone()]);
    let second = biblatex(&[a, z]);
    assert_eq!(first, second);
    assert!(
        first.find("@article{alpha2024").unwrap() < first.find("@inproceedings{zeta2024").unwrap()
    );
    for expected in [
        "Müller, Jane and Curie, François",
        "A \\& B \\% C \\$ D \\# E \\_ \\{F\\}",
        "\\textasciitilde{}",
        "\\textasciicircum{}",
        "\\textbackslash{}",
        "pages        = {313--323}",
        "url          = {https://example.test/a\\_b}",
    ] {
        assert!(first.contains(expected), "missing {expected:?} in {first}");
    }
    assert!(first.contains("booktitle"));
    assert!(first.contains("journaltitle"));
}

// Scenario: non numeric page text is preserved instead of blindly normalized.
// Requirement: REQ-052
#[test]
fn non_numeric_page_text_is_preserved_instead_of_blindly_normalized() {
    let mut item = stored("pages");
    item.metadata.pages = Some("S1-S4-extra".into());
    assert!(biblatex(&[item]).contains("pages        = {S1-S4-extra}"));
}
