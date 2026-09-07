mod support;

use anyhow::{bail, Result};
use r#ref::{
    launch::{edit_reference, open_reference, Editor, Environment, FileOpener},
    model::CitationKey,
    repository::Repository,
};
use std::{
    cell::RefCell,
    collections::HashMap,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

#[derive(Default)]
struct RecordingOpener(RefCell<Vec<PathBuf>>);
impl FileOpener for RecordingOpener {
    fn open(&self, path: &Path) -> Result<()> {
        self.0.borrow_mut().push(path.to_owned());
        Ok(())
    }
}
struct FailingOpener;
impl FileOpener for FailingOpener {
    fn open(&self, _: &Path) -> Result<()> {
        bail!("viewer unavailable")
    }
}

#[derive(Default)]
struct MapEnvironment(HashMap<String, OsString>);
impl Environment for MapEnvironment {
    fn variable(&self, name: &str) -> Option<OsString> {
        self.0.get(name).cloned()
    }
}
#[derive(Default)]
struct RecordingEditor(RefCell<Vec<(String, PathBuf)>>);
impl Editor for RecordingEditor {
    fn edit(&self, command: &str, path: &Path) -> Result<bool> {
        self.0.borrow_mut().push((command.into(), path.into()));
        Ok(true)
    }
}

// Scenario: open requests the references pdf without launching a desktop app.
// Requirement: REQ-024
#[test]
fn open_requests_the_references_pdf_without_launching_a_desktop_app() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path()).unwrap();
    let key = CitationKey::new("paper2024").unwrap();
    let pdf = temp.path().join("input.pdf");
    fs::write(&pdf, b"pdf").unwrap();
    repo.add(
        &key,
        &support::sample("Paper", "Smith", Some(2024)),
        Some(&pdf),
    )
    .unwrap();
    let opener = RecordingOpener::default();

    open_reference(&repo, &key, &opener).unwrap();

    assert_eq!(
        &*opener.0.borrow(),
        &[repo.reference_path(&key).join("paper.pdf")]
    );
    assert!(open_reference(&repo, &key, &FailingOpener)
        .unwrap_err()
        .to_string()
        .contains("failed to open"));
}

// Scenario: open does not invoke opener for missing pdf or key.
// Requirement: REQ-025
#[test]
fn open_does_not_invoke_opener_for_missing_pdf_or_key() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path()).unwrap();
    let key = CitationKey::new("no_pdf").unwrap();
    support::add(
        &repo,
        key.as_str(),
        &support::sample("No PDF", "Smith", None),
    );
    let opener = RecordingOpener::default();
    assert!(open_reference(&repo, &key, &opener).is_err());
    assert!(open_reference(&repo, &CitationKey::new("unknown").unwrap(), &opener).is_err());
    assert!(opener.0.borrow().is_empty());
}

// Scenario: edit prefers visual and targets metadata without global environment mutation.
// Requirement: REQ-026
#[test]
fn edit_prefers_visual_and_targets_metadata_without_global_environment_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path()).unwrap();
    let key = CitationKey::new("edit2024").unwrap();
    support::add(
        &repo,
        key.as_str(),
        &support::sample("Edit", "Smith", Some(2024)),
    );
    let environment = MapEnvironment(HashMap::from([
        ("VISUAL".into(), OsString::from("visual --wait")),
        ("EDITOR".into(), OsString::from("editor")),
    ]));
    let editor = RecordingEditor::default();
    edit_reference(&repo, &key, &environment, &editor).unwrap();
    assert_eq!(
        &*editor.0.borrow(),
        &[(
            "visual --wait".into(),
            repo.reference_path(&key).join("ref.yaml")
        )]
    );
}

// Scenario: edit requires configuration and preserves invalid user edits.
// Requirement: REQ-027
#[test]
fn edit_requires_configuration_and_preserves_invalid_user_edits() {
    struct InvalidatingEditor;
    impl Editor for InvalidatingEditor {
        fn edit(&self, _: &str, path: &Path) -> Result<bool> {
            fs::write(path, "not: [valid").unwrap();
            Ok(true)
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path()).unwrap();
    let key = CitationKey::new("edit2024").unwrap();
    support::add(
        &repo,
        key.as_str(),
        &support::sample("Edit", "Smith", Some(2024)),
    );
    assert!(edit_reference(
        &repo,
        &key,
        &MapEnvironment::default(),
        &RecordingEditor::default()
    )
    .is_err());
    let environment = MapEnvironment(HashMap::from([("EDITOR".into(), OsString::from("editor"))]));
    assert!(edit_reference(&repo, &key, &environment, &InvalidatingEditor).is_err());
    assert_eq!(
        fs::read_to_string(repo.reference_path(&key).join("ref.yaml")).unwrap(),
        "not: [valid"
    );
}
