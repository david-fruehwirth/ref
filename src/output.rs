use anyhow::Result;
use serde::Serialize;
use std::{io::IsTerminal, io::Write, path::PathBuf};

pub const SCHEMA_VERSION: u8 = 1;

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum OutputFormat {
    Human,
    Json,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationStatus {
    Success,
    PartialSuccess,
    Failure,
}

#[derive(Clone, Serialize)]
pub struct WarningOutput {
    pub warning_code: String,
    pub message: String,
    pub citation_key: Option<String>,
}

#[derive(Serialize)]
struct SuccessEnvelope<'a, T> {
    schema_version: u8,
    command: &'a str,
    operation_status: OperationStatus,
    warnings: &'a [WarningOutput],
    result: &'a T,
}

#[derive(Serialize)]
struct FailureEnvelope<'a> {
    schema_version: u8,
    command: &'a str,
    operation_status: &'static str,
    error: ErrorOutput,
}

#[derive(Serialize)]
pub struct ErrorOutput {
    pub error_code: String,
    pub message: String,
    pub citation_key: Option<String>,
    pub hint: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub missing_fields: Vec<String>,
}

pub trait HumanRenderable {
    fn render_human(&self, writer: &mut dyn Write) -> Result<()>;

    fn render_human_with_color(&self, writer: &mut dyn Write, _color: bool) -> Result<()> {
        self.render_human(writer)
    }

    fn render_human_stderr(&self, _writer: &mut dyn Write) -> Result<()> {
        Ok(())
    }
}

pub fn render<T: Serialize + HumanRenderable>(
    format: OutputFormat,
    command: &str,
    status: OperationStatus,
    warnings: &[WarningOutput],
    value: &T,
) -> Result<()> {
    match format {
        OutputFormat::Human => {
            for warning in warnings {
                eprintln!("warning: {}", warning.message);
            }
            value.render_human_stderr(&mut std::io::stderr())?;
            let color = std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
            value.render_human_with_color(&mut std::io::stdout(), color)
        }
        OutputFormat::Json => {
            serde_json::to_writer_pretty(
                std::io::stdout(),
                &SuccessEnvelope {
                    schema_version: SCHEMA_VERSION,
                    command,
                    operation_status: status,
                    warnings,
                    result: value,
                },
            )?;
            writeln!(std::io::stdout())?;
            Ok(())
        }
    }
}

pub fn render_error(format: OutputFormat, command: &str, error: &anyhow::Error) {
    if format == OutputFormat::Human {
        eprintln!("error: {error:#}");
        return;
    }
    let message = format!("{error:#}");
    let citation_key = extract_between(&message, "reference `", "`");
    let (error_code, hint, missing_fields) = classify_error(&message);
    let envelope = FailureEnvelope {
        schema_version: SCHEMA_VERSION,
        command,
        operation_status: "failure",
        error: ErrorOutput {
            error_code: error_code.into(),
            message,
            citation_key,
            hint,
            missing_fields,
        },
    };
    let _ = serde_json::to_writer_pretty(std::io::stderr(), &envelope);
    let _ = writeln!(std::io::stderr());
}

fn extract_between(value: &str, prefix: &str, suffix: &str) -> Option<String> {
    let rest = value.split_once(prefix)?.1;
    Some(rest.split_once(suffix)?.0.to_owned())
}

fn classify_error(message: &str) -> (&'static str, Option<String>, Vec<String>) {
    if message.contains("confirmation required") {
        return (
            "confirmation_required",
            Some("Use --yes to confirm the operation.".into()),
            vec![],
        );
    }
    if message.contains("does not exist") && message.contains("reference `") {
        return ("reference_not_found", None, vec![]);
    }
    if message.contains("invalid citation key") {
        return ("invalid_citation_key", None, vec![]);
    }
    if message.contains("not inside a ref repository") {
        return (
            "repository_not_found",
            Some("Run `ref init` to create one.".into()),
            vec![],
        );
    }
    if message.contains("already exists") {
        return ("reference_already_exists", None, vec![]);
    }
    if message.contains("source PDF") {
        return ("source_pdf_missing", None, vec![]);
    }
    if message.contains("--title is required") {
        return ("missing_required_metadata", None, vec!["title".into()]);
    }
    if message.contains("failed to parse") {
        return ("bibliography_parse_failed", None, vec![]);
    }
    if message.contains("clean scan incomplete") {
        return ("clean_scan_incomplete", None, vec![]);
    }
    if message.contains("invalid author") {
        return ("invalid_author", None, vec![]);
    }
    ("command_failed", None, vec![])
}

#[derive(Clone, Serialize)]
pub struct AuthorOutput {
    pub given_names: String,
    pub family_name: String,
}

#[derive(Clone, Serialize)]
pub struct ReferenceOutput {
    pub citation_key: String,
    pub reference_type: String,
    pub title: String,
    pub authors: Vec<AuthorOutput>,
    pub publication_year: Option<u16>,
    pub container_title: Option<String>,
    pub publisher: Option<String>,
    pub volume: Option<String>,
    pub issue: Option<String>,
    pub pages: Option<String>,
    pub doi: Option<String>,
    pub url: Option<String>,
    pub tags: Vec<String>,
    pub notes: Option<String>,
    pub source_pdf_present: bool,
    pub source_pdf_path: Option<PathBuf>,
}

#[derive(Clone, Serialize)]
pub struct RecentReferenceOutput {
    pub citation_key: String,
}

#[derive(Clone, Serialize)]
pub struct PdfMigrationOutput {
    pub citation_key: String,
    pub source_path: PathBuf,
    pub destination_path: PathBuf,
}

impl ReferenceOutput {
    pub fn from_stored(r: &r#ref::repository::StoredReference) -> Self {
        let m = &r.metadata;
        Self {
            citation_key: r.key.to_string(),
            reference_type: m.entry_type.to_string(),
            title: m.title.clone(),
            authors: m
                .authors
                .iter()
                .map(|a| AuthorOutput {
                    given_names: a.given.clone(),
                    family_name: a.family.clone(),
                })
                .collect(),
            publication_year: m.year,
            container_title: m.container_title.clone(),
            publisher: m.publisher.clone(),
            volume: m.volume.clone(),
            issue: m.issue.clone(),
            pages: m.pages.clone(),
            doi: m.doi.clone(),
            url: m.url.clone(),
            tags: m.tags.clone(),
            notes: m.notes.clone(),
            source_pdf_present: r.has_pdf,
            source_pdf_path: r.has_pdf.then(|| r.pdf_path.clone()).flatten(),
        }
    }
}

#[derive(Serialize)]
#[serde(untagged)]
pub enum CommandOutput {
    Init {
        repository_root_path: PathBuf,
        reference_repository_path: PathBuf,
        repository_format_version: u8,
    },
    Add {
        created_reference: ReferenceOutput,
        reference_directory_path: PathBuf,
    },
    List {
        reference_count: usize,
        references: Vec<ReferenceOutput>,
    },
    Last {
        references: Vec<RecentReferenceOutput>,
    },
    Search {
        search_query: String,
        search_field: String,
        matching_reference_count: usize,
        matching_references: Vec<ReferenceOutput>,
    },
    Show(ReferenceOutput),
    Open {
        citation_key: String,
        source_pdf_path: PathBuf,
        external_viewer_launch_requested: bool,
    },
    Attach {
        citation_key: String,
        source_pdf_path: PathBuf,
    },
    Edit {
        citation_key: String,
        metadata_file_path: PathBuf,
        metadata_valid_after_edit: bool,
    },
    Rename {
        previous_citation_key: String,
        new_citation_key: String,
        reference_directory_path: PathBuf,
    },
    Remove {
        removed_citation_key: String,
        reference_removed: bool,
        source_pdf_removed: bool,
    },
    Export {
        export_format: String,
        reference_count: usize,
        bibliography_content: Option<String>,
        output_file_path: Option<PathBuf>,
        bibliography_written_to_file: bool,
    },
    Import {
        input_bibliography_path: PathBuf,
        entries_found: usize,
        references_imported: usize,
        references_skipped: usize,
        references_failed: usize,
        imported_citation_keys: Vec<String>,
        skipped_references: Vec<BatchDiagnostic>,
        failed_references: Vec<BatchDiagnostic>,
    },
    MigratePdfs {
        dry_run: bool,
        changes: Vec<PdfMigrationOutput>,
    },
    Clean {
        dry_run: bool,
        repository_root_path: PathBuf,
        scan_root_path: PathBuf,
        eligible_files_scanned: usize,
        reference_count: usize,
        used_reference_count: usize,
        unused_reference_count: usize,
        unused_references: Vec<ReferenceOutput>,
        references_removed: Vec<String>,
        failed_references: Vec<BatchDiagnostic>,
    },
    Doctor {
        repository_root_path: PathBuf,
        reference_count: usize,
        references_with_source_pdf: usize,
        references_without_source_pdf: usize,
        warning_count: usize,
        error_count: usize,
        strict_mode: bool,
        diagnostics: Vec<DiagnosticOutput>,
    },
}

#[derive(Clone, Serialize)]
pub struct BatchDiagnostic {
    pub citation_key: String,
    pub diagnostic_code: String,
    pub message: String,
}
#[derive(Clone, Serialize)]
pub struct DiagnosticOutput {
    pub severity: String,
    pub diagnostic_code: String,
    pub citation_key: Option<String>,
    pub message: String,
}

impl HumanRenderable for CommandOutput {
    fn render_human(&self, w: &mut dyn Write) -> Result<()> {
        match self {
            Self::Init {
                reference_repository_path,
                ..
            } => writeln!(
                w,
                "Initialized reference repository at {}",
                reference_repository_path.display()
            )?,
            Self::Add {
                created_reference, ..
            } => writeln!(w, "Added {}", created_reference.citation_key)?,
            Self::List { references, .. }
            | Self::Search {
                matching_references: references,
                ..
            } => reference_summaries(w, references, false)?,
            Self::Last { references } => {
                for reference in references {
                    writeln!(w, "{}", reference.citation_key)?;
                }
            }
            Self::Show(r) => show(w, r)?,
            Self::Open { .. } => (),
            Self::Attach { citation_key, .. } => {
                writeln!(w, "Attached source PDF to {citation_key}")?
            }
            Self::Edit { .. } => (),
            Self::Rename {
                previous_citation_key,
                new_citation_key,
                ..
            } => writeln!(w, "Renamed {previous_citation_key} → {new_citation_key}")?,
            Self::Remove {
                removed_citation_key,
                reference_removed,
                ..
            } if *reference_removed => writeln!(w, "Removed {removed_citation_key}")?,
            Self::Remove { .. } => (),
            Self::Export {
                bibliography_content: Some(content),
                ..
            } => write!(w, "{content}")?,
            Self::Export { .. } => (),
            Self::Import {
                input_bibliography_path,
                entries_found,
                references_imported,
                references_skipped,
                references_failed,
                ..
            } => {
                if *references_skipped == 0 && *references_failed == 0 {
                    writeln!(
                        w,
                        "Imported {references_imported} references from {}",
                        input_bibliography_path.display()
                    )?;
                } else {
                    writeln!(w, "Import complete with errors.\n\nEntries:  {entries_found}\nImported: {references_imported}\nSkipped:  {references_skipped}\nFailed:   {references_failed}")?;
                }
            }
            Self::MigratePdfs { dry_run, changes } => {
                for change in changes {
                    writeln!(
                        w,
                        "{}: {} -> {}",
                        change.citation_key,
                        change.source_path.display(),
                        change.destination_path.display()
                    )?;
                }
                if *dry_run {
                    writeln!(
                        w,
                        "Dry run: {} PDF(s) would be migrated; no files were changed.",
                        changes.len()
                    )?;
                } else {
                    writeln!(w, "Migrated {} PDF(s).", changes.len())?;
                }
            }
            Self::Clean {
                dry_run,
                repository_root_path,
                scan_root_path,
                eligible_files_scanned,
                reference_count,
                used_reference_count,
                unused_reference_count,
                unused_references,
                references_removed,
                ..
            } => {
                writeln!(w, "Repository:\n  {}\n\nScan root:\n  {}\n\nFiles scanned: {eligible_files_scanned}\nReferences:    {reference_count}\nUsed:          {used_reference_count}\nUnused:        {unused_reference_count}", repository_root_path.display(), scan_root_path.display())?;
                let project_root = repository_root_path
                    .parent()
                    .unwrap_or(repository_root_path);
                if scan_root_path != project_root {
                    writeln!(
                        w,
                        "\nNote: files outside this directory were not considered."
                    )?;
                }
                if !unused_references.is_empty() {
                    writeln!(
                        w,
                        "\n{}:",
                        if *dry_run {
                            "Would remove"
                        } else {
                            "Unused references"
                        }
                    )?;
                    for r in unused_references {
                        writeln!(w, "\n  {}\n    {}", r.citation_key, r.title)?;
                    }
                }
                if *dry_run {
                    writeln!(w, "\nDry run: no references were removed.")?;
                } else if !references_removed.is_empty() {
                    writeln!(w, "Removed {} unused references.", references_removed.len())?;
                } else if *unused_reference_count > 0 {
                    writeln!(w, "Cleanup cancelled. No references were removed.")?;
                } else {
                    writeln!(w, "\nNothing to clean.")?;
                }
            }
            Self::Doctor {
                repository_root_path,
                reference_count,
                references_with_source_pdf,
                warning_count,
                error_count,
                diagnostics,
                ..
            } => {
                writeln!(w, "Repository: {}\n\n✓ configuration valid\n✓ {reference_count} references discovered\n{} {references_with_source_pdf} / {reference_count} references have source PDFs", repository_root_path.display(), if *error_count == 0 && *warning_count == 0 { "✓" } else { "!" })?;
                for heading in ["warning", "error"] {
                    let ds: Vec<_> = diagnostics
                        .iter()
                        .filter(|d| d.severity == heading)
                        .collect();
                    if !ds.is_empty() {
                        writeln!(
                            w,
                            "\n{}s:",
                            if heading == "warning" {
                                "Warning"
                            } else {
                                "Error"
                            }
                        )?;
                        for d in ds {
                            writeln!(w, "  {}", d.message)?;
                        }
                    }
                }
                writeln!(w, "\n{reference_count} references, {warning_count} warnings, {error_count} errors")?;
            }
        }
        Ok(())
    }

    fn render_human_with_color(&self, w: &mut dyn Write, color: bool) -> Result<()> {
        match self {
            Self::List { references, .. }
            | Self::Search {
                matching_references: references,
                ..
            } => reference_summaries(w, references, color),
            _ => self.render_human(w),
        }
    }

    fn render_human_stderr(&self, w: &mut dyn Write) -> Result<()> {
        match self {
            Self::Import {
                skipped_references,
                failed_references,
                ..
            } => {
                for diagnostic in skipped_references {
                    writeln!(
                        w,
                        "warning: skipped `{}`\n  {}",
                        diagnostic.citation_key, diagnostic.message
                    )?;
                }
                for diagnostic in failed_references {
                    writeln!(
                        w,
                        "warning: failed to import `{}`\n  {}",
                        diagnostic.citation_key, diagnostic.message
                    )?;
                }
            }
            Self::Clean {
                failed_references, ..
            } if !failed_references.is_empty() => {
                writeln!(w, "\nFailed to remove:")?;
                for diagnostic in failed_references {
                    writeln!(w, "  {}: {}", diagnostic.citation_key, diagnostic.message)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

fn reference_summaries(
    w: &mut dyn Write,
    references: &[ReferenceOutput],
    color: bool,
) -> Result<()> {
    for (index, reference) in references.iter().enumerate() {
        if index > 0 {
            writeln!(w)?;
        }
        styled_line(w, &reference.citation_key, "1;36", color)?;
        summary_field(w, "Title:", &reference.title, color)?;
        if !reference.authors.is_empty() {
            let authors = reference
                .authors
                .iter()
                .map(|author| format!("{} {}", author.given_names, author.family_name))
                .collect::<Vec<_>>()
                .join(", ");
            summary_field(w, "Authors:", &authors, color)?;
        }
        if let Some(year) = reference.publication_year {
            summary_field(w, "Year:", &year.to_string(), color)?;
        }
        summary_field(w, "Type:", &reference.reference_type, color)?;
    }
    Ok(())
}

fn styled_line(w: &mut dyn Write, value: &str, style: &str, color: bool) -> Result<()> {
    if color {
        writeln!(w, "\x1b[{style}m{value}\x1b[0m")?;
    } else {
        writeln!(w, "{value}")?;
    }
    Ok(())
}

fn summary_field(w: &mut dyn Write, label: &str, value: &str, color: bool) -> Result<()> {
    write!(w, "    ")?;
    if color {
        write!(w, "\x1b[36m{label:<9}\x1b[0m")?;
    } else {
        write!(w, "{label:<9}")?;
    }
    writeln!(w, "{value}")?;
    Ok(())
}

fn show(w: &mut dyn Write, r: &ReferenceOutput) -> Result<()> {
    writeln!(
        w,
        "Key:       {}\nType:      {}\nTitle:     {}",
        r.citation_key, r.reference_type, r.title
    )?;
    writeln!(
        w,
        "Authors:   {}",
        r.authors
            .iter()
            .map(|a| format!("{} {}", a.given_names, a.family_name))
            .collect::<Vec<_>>()
            .join(", ")
    )?;
    if let Some(y) = r.publication_year {
        writeln!(w, "Year:      {y}")?;
    }
    for (n, v) in [
        ("Container", &r.container_title),
        ("Publisher", &r.publisher),
        ("Volume", &r.volume),
        ("Issue", &r.issue),
        ("Pages", &r.pages),
        ("DOI", &r.doi),
        ("URL", &r.url),
    ] {
        if let Some(v) = v {
            writeln!(w, "{n}: {v}")?;
        }
    }
    if !r.tags.is_empty() {
        writeln!(w, "Tags:      {}", r.tags.join(", "))?;
    }
    writeln!(
        w,
        "PDF:       {}",
        if r.source_pdf_present { "yes" } else { "no" }
    )?;
    if let Some(n) = &r.notes {
        writeln!(w, "\nNotes:\n{n}")?;
    }
    Ok(())
}

#[cfg(test)]
mod reference_summary_tests {
    use super::*;

    fn reference() -> ReferenceOutput {
        ReferenceOutput {
            citation_key: "Smith2024".into(),
            reference_type: "article".into(),
            title: "Attention".into(),
            authors: vec![],
            publication_year: None,
            container_title: None,
            publisher: None,
            volume: None,
            issue: None,
            pages: None,
            doi: None,
            url: None,
            tags: vec![],
            notes: None,
            source_pdf_present: false,
            source_pdf_path: None,
        }
    }

    // Scenario: explicit color styling emphasizes summary keys and labels only.
    // Requirement: REQ-074
    #[test]
    fn color_enabled_summary_styles_semantic_elements() {
        let mut output = Vec::new();
        reference_summaries(&mut output, &[reference()], true).unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.starts_with("\x1b[1;36mSmith2024\x1b[0m\n"));
        assert!(output.contains("    \x1b[36mTitle:   \x1b[0mAttention\n"));
        assert!(output.contains("    \x1b[36mType:    \x1b[0marticle\n"));
    }
}
