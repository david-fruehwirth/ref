//! Repository integrity and quality diagnostics.
//!
//! Doctor deliberately covers both unusable structure and usable-but-incomplete
//! scientific records. In particular, bibliographic metadata is valid without
//! source evidence, while the absence of that evidence is a quality warning.

use crate::{
    metadata::Doi,
    model::{CitationKey, Reference},
    repository::{sha256_file, Repository, SourceProofStatus},
    url_check::{UrlChecker, UrlStatus},
};
use anyhow::Result;
use std::{collections::HashMap, fs};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticSeverity {
    Info,
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
    MissingPdfHash { key: CitationKey },
    PdfHashMismatch { key: CitationKey },
    UrlStatus { key: CitationKey, status: UrlStatus },
}

impl DoctorDiagnostic {
    pub fn severity(&self) -> DiagnosticSeverity {
        match self {
            Self::UrlStatus { status, .. } if status.is_reachable() => DiagnosticSeverity::Info,
            Self::UrlStatus { .. } => DiagnosticSeverity::Warning,
            Self::MissingYear { .. }
            | Self::MissingAuthors { .. }
            | Self::DuplicateDoi { .. }
            | Self::MissingSourcePdf { .. }
            | Self::MissingPdfHash { .. } => DiagnosticSeverity::Warning,
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
            Self::MissingPdfHash { key } => format!(
                "{key}: source PDF has no integrity hash; run `ref hash` to create it"
            ),
            Self::PdfHashMismatch { key } => format!(
                "{key}: source PDF integrity mismatch; verify the PDF, then run `ref hash` to refresh its hash"
            ),
            Self::UrlStatus { key, status } => format!("{key}: URL {}", url_status_message(status)),
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
    inspect_with_options(repo, true, &crate::url_check::HttpUrlChecker::default())
}

pub fn inspect_with_options(
    repo: &Repository,
    check_urls: bool,
    checker: &dyn UrlChecker,
) -> Result<DoctorReport> {
    repo.validate_structure()?;
    let hashing = repo.pdf_hashing()?;
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
        let (metadata, _, _, stored_hash): (Reference, _, _, _) =
            match crate::repository::parse_reference_yaml(&text) {
                Ok(parsed) => parsed,
                Err(error) => {
                    report.diagnostics.push(DoctorDiagnostic::InvalidMetadata {
                        key: key.to_string(),
                        reason: error.to_string(),
                    });
                    continue;
                }
            };
        if hashing {
            if let SourceProofStatus::Present(path) = repo.source_proof_status(&key) {
                match stored_hash {
                    Some(expected) => match sha256_file(&path) {
                        Ok(actual) if actual != expected => report
                            .diagnostics
                            .push(DoctorDiagnostic::PdfHashMismatch { key: key.clone() }),
                        Ok(_) => {}
                        Err(error) => report.diagnostics.push(DoctorDiagnostic::InvalidSourcePdf {
                            key: key.clone(),
                            reason: error.to_string(),
                        }),
                    },
                    None => report
                        .diagnostics
                        .push(DoctorDiagnostic::MissingPdfHash { key: key.clone() }),
                }
            }
        }
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
                Ok(doi) => dois.entry(doi.to_string()).or_default().push(key.clone()),
                Err(_) => report
                    .diagnostics
                    .push(DoctorDiagnostic::MalformedDoi { key: key.clone() }),
            }
        }
        if check_urls {
            if let Some(url) = metadata.url.as_deref() {
                let status = checker
                    .check(url)
                    .unwrap_or_else(|error| UrlStatus::ConnectionFailure(error.to_string()));
                report
                    .diagnostics
                    .push(DoctorDiagnostic::UrlStatus { key, status });
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

fn url_status_message(status: &UrlStatus) -> String {
    match status {
        UrlStatus::Reachable {
            final_url,
            redirected: true,
        } => format!("redirected; reachable at {final_url}"),
        UrlStatus::Reachable { .. } => "reachable".into(),
        UrlStatus::ClientError(code) => format!("client error (HTTP {code})"),
        UrlStatus::ServerError(code) => format!("server error (HTTP {code})"),
        UrlStatus::Timeout => "timeout".into(),
        UrlStatus::ConnectionFailure(reason) => format!("connection/DNS/TLS failure ({reason})"),
    }
}

#[cfg(test)]
mod url_tests {
    use super::*;
    use crate::{
        model::{AccessDate, ReferenceType},
        repository::Repository,
    };
    struct Fixed(UrlStatus);
    impl UrlChecker for Fixed {
        fn check(&self, _: &str) -> Result<UrlStatus> {
            Ok(self.0.clone())
        }
    }
    fn repo_with_url() -> (tempfile::TempDir, Repository) {
        let t = tempfile::tempdir().unwrap();
        let repo = Repository::init(t.path()).unwrap();
        let r = Reference {
            entry_type: ReferenceType::Online,
            title: "Page".into(),
            authors: vec![],
            year: None,
            date: None,
            container_title: None,
            publisher: None,
            volume: None,
            issue: None,
            pages: None,
            doi: None,
            url: Some("https://example.org".into()),
            urldate: Some(AccessDate::new("2026-09-19").unwrap()),
            tags: vec![],
            notes: None,
        };
        repo.add(&CitationKey::new("page").unwrap(), &r, None)
            .unwrap();
        (t, repo)
    }
    // Scenario: doctor distinguishes a reachable stored URL as informational. Requirement: REQ-117.
    #[test]
    fn reports_reachable_url() {
        let (_t, r) = repo_with_url();
        let report = inspect_with_options(
            &r,
            true,
            &Fixed(UrlStatus::Reachable {
                final_url: "https://example.org".into(),
                redirected: false,
            }),
        )
        .unwrap();
        assert!(report.diagnostics.iter().any(|d| matches!(
            d,
            DoctorDiagnostic::UrlStatus {
                status: UrlStatus::Reachable { .. },
                ..
            }
        ) && d.severity() == DiagnosticSeverity::Info));
    }
    // Scenario: doctor reports an unreachable stored URL as a strict-mode warning. Requirement: REQ-117.
    #[test]
    fn reports_unreachable_url() {
        let (_t, r) = repo_with_url();
        let report = inspect_with_options(&r, true, &Fixed(UrlStatus::ServerError(503))).unwrap();
        assert!(report.diagnostics.iter().any(|d| matches!(
            d,
            DoctorDiagnostic::UrlStatus {
                status: UrlStatus::ServerError(503),
                ..
            }
        ) && d.severity()
            == DiagnosticSeverity::Warning));
    }
}
