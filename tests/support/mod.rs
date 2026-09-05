use assert_cmd::Command;
use r#ref::{
    model::{CitationKey, Person, Reference, ReferenceType},
    repository::Repository,
};
use std::path::Path;

// Each integration-test file is compiled as a separate crate, so a helper used by
// one suite can legitimately be unused by another suite importing this module.
#[allow(dead_code)]
pub fn command(dir: &Path) -> Command {
    let mut command = Command::cargo_bin("ref").expect("compiled ref binary");
    command
        .current_dir(dir)
        .env_remove("VISUAL")
        .env_remove("EDITOR")
        .env("NO_COLOR", "1");
    command
}

#[allow(dead_code)]
pub fn sample(title: &str, family: &str, year: Option<u16>) -> Reference {
    Reference {
        entry_type: ReferenceType::Article,
        title: title.into(),
        authors: vec![Person {
            given: "Jane".into(),
            family: family.into(),
        }],
        year,
        container_title: Some("Journal of Examples".into()),
        publisher: None,
        volume: None,
        issue: None,
        pages: None,
        doi: None,
        url: None,
        tags: vec![],
        notes: None,
    }
}

#[allow(dead_code)]
pub fn add(repo: &Repository, key: &str, reference: &Reference) {
    repo.add(&CitationKey::new(key).unwrap(), reference, None)
        .unwrap();
}
