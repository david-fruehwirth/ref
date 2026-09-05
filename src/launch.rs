//! Testable boundaries around desktop applications.

use crate::{model::CitationKey, repository::Repository};
use anyhow::{anyhow, bail, Context, Result};
use std::{ffi::OsString, path::Path};

pub trait FileOpener {
    fn open(&self, path: &Path) -> Result<()>;
}

pub trait Environment {
    fn variable(&self, name: &str) -> Option<OsString>;
}

pub trait Editor {
    /// Returns whether the editor exited successfully.
    fn edit(&self, command: &str, path: &Path) -> Result<bool>;
}

pub fn open_reference(repo: &Repository, key: &CitationKey, opener: &dyn FileOpener) -> Result<()> {
    let reference = repo.load_reference(key)?;
    let pdf = reference.path.join("paper.pdf");
    if !pdf.is_file() {
        bail!("reference `{key}` has no PDF");
    }
    opener
        .open(&pdf)
        .with_context(|| format!("failed to open {}", pdf.display()))
}

pub fn edit_reference(
    repo: &Repository,
    key: &CitationKey,
    environment: &dyn Environment,
    editor: &dyn Editor,
) -> Result<()> {
    let reference = repo.load_reference(key)?;
    let command = environment
        .variable("VISUAL")
        .or_else(|| environment.variable("EDITOR"))
        .ok_or_else(|| {
            anyhow!("no editor configured\nhint: set the VISUAL or EDITOR environment variable")
        })?;
    let command = command
        .into_string()
        .map_err(|_| anyhow!("editor command is not valid UTF-8"))?;
    if command.trim().is_empty() {
        bail!("editor command is empty");
    }
    if !editor.edit(&command, &reference.path.join("ref.yaml"))? {
        bail!("editor exited unsuccessfully");
    }
    // Validation happens after the editor exits, without rewriting the user's file.
    repo.load_reference(key)?;
    Ok(())
}
