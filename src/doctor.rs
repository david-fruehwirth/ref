//! Repository integrity and quality diagnostics.
//!
//! Doctor deliberately covers both unusable structure and usable-but-incomplete
//! scientific records. In particular, bibliographic metadata is valid without
//! source evidence, while the absence of that evidence is a quality warning.

use crate::{
    metadata::Doi,
    model::{CitationKey, Reference},
    repository::{Repository, SourceProofStatus},
};
use anyhow::Result;
use std::{collections::HashMap, fs};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticSeverity {
    Warning,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DoctorDiagnostic {
    InvalidCitationKey { name: String },
    MissingMetadata { key: String },
    InvalidMetadata { key: String, reason: String },
    MissingYear { key: CitationKey },
    MissingAuthors { key: CitationKey },
    MalformedDoi { key: CitationKey },
    DuplicateDoi { keys: Vec<CitationKey> },
    MissingSourcePdf { key: CitationKey },
    InvalidSourcePdf { key: CitationKey, reason: String },
}

impl DoctorDiagnostic {
    pub fn severity(&self) -> DiagnosticSeverity {
        match self {
            Self::MissingYear { .. }
            | Self::MissingAuthors { .. }
            | Self::DuplicateDoi { .. }
            | Self::MissingSourcePdf { .. } => DiagnosticSeverity::Warning,
            _ => DiagnosticSeverity::Error,
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::InvalidCitationKey { name } => format!("{name}: invalid citation key"),
            Self::MissingMetadata { key } => format!("{key}: ref.yaml missing"),
            Self::InvalidMetadata { key, reason } => {
                format!("{key}: invalid metadata: {reason}")
            }
            Self::MissingYear { key } => format!("{key}: year missing"),
            Self::MissingAuthors { key } => format!("{key}: authors missing"),
            Self::MalformedDoi { key } => format!("{key}: malformed DOI"),
            Self::DuplicateDoi { keys } => format!(
                "duplicate DOI: {}",
                keys.iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::MissingSourcePdf { key } => format!("{key}: source PDF missing"),
            Self::InvalidSourcePdf { key, reason } => {
                format!("{key}: invalid source PDF: {reason}")
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct DoctorReport {
    pub references_total: usize,
    pub metadata_parsed: usize,
    pub references_with_source_pdf: usize,
    pub references_without_source_pdf: usize,
    pub diagnostics: Vec<DoctorDiagnostic>,
}

impl DoctorReport {
    pub fn warning_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity() == DiagnosticSeverity::Warning)
            .count()
    }

    pub fn error_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity() == DiagnosticSeverity::Error)
            .count()
    }
}

pub fn inspect(repo: &Repository) -> Result<DoctorReport> {
    repo.validate_structure()?;
    let mut report = DoctorReport::default();
    let mut dois: HashMap<String, Vec<CitationKey>> = HashMap::new();
    // Filesystem iteration order is unspecified. Sort before validation so both
    // structured diagnostics and human-readable doctor output are deterministic.
    let mut entries = fs::read_dir(repo.references_dir())?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        if !entry.file_type()?.is_dir() {
            continue;
        }
        report.references_total += 1;
        let name = entry.file_name().to_string_lossy().into_owned();
        let key = match CitationKey::new(&name) {
            Ok(key) => key,
            Err(_) => {
                report
                    .diagnostics
                    .push(DoctorDiagnostic::InvalidCitationKey { name });
                continue;
            }
        };

        match repo.source_proof_status(&key) {
            SourceProofStatus::Present(_) => report.references_with_source_pdf += 1,
            SourceProofStatus::Missing => {
                report.references_without_source_pdf += 1;
                report
                    .diagnostics
                    .push(DoctorDiagnostic::MissingSourcePdf { key: key.clone() });
            }
            SourceProofStatus::Invalid(reason) => {
                report.diagnostics.push(DoctorDiagnostic::InvalidSourcePdf {
                    key: key.clone(),
                    reason,
                })
            }
        }

        let yaml = entry.path().join("ref.yaml");
        if !yaml.is_file() {
            report.diagnostics.push(DoctorDiagnostic::MissingMetadata {
                key: key.to_string(),
            });
            continue;
        }
        let text = match fs::read_to_string(&yaml) {
            Ok(text) => text,
            Err(error) => {
                report.diagnostics.push(DoctorDiagnostic::InvalidMetadata {
                    key: key.to_string(),
                    reason: format!("cannot read metadata: {error}"),
                });
                continue;
            }
        };
        let metadata: Reference = match crate::repository::parse_reference_yaml(&text) {
            Ok((metadata, _)) => metadata,
            Err(error) => {
                report.diagnostics.push(DoctorDiagnostic::InvalidMetadata {
                    key: key.to_string(),
                    reason: error.to_string(),
                });
                continue;
            }
        };
        report.metadata_parsed += 1;
        if let Err(error) = metadata.validate() {
            report.diagnostics.push(DoctorDiagnostic::InvalidMetadata {
                key: key.to_string(),
                reason: error.to_string(),
            });
        }
        if metadata.year.is_none() {
            report
                .diagnostics
                .push(DoctorDiagnostic::MissingYear { key: key.clone() });
        }
        if metadata.authors.is_empty() {
            report
                .diagnostics
                .push(DoctorDiagnostic::MissingAuthors { key: key.clone() });
        }
        if let Some(doi) = metadata.doi {
            match doi.parse::<Doi>() {
                Ok(doi) => dois.entry(doi.to_string()).or_default().push(key),
                Err(_) => report
                    .diagnostics
                    .push(DoctorDiagnostic::MalformedDoi { key }),
            }
        }
    }
    for keys in dois.into_values().filter(|keys| keys.len() > 1) {
        report
            .diagnostics
            .push(DoctorDiagnostic::DuplicateDoi { keys });
    }
    Ok(report)
}
