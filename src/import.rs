//! BibTeX/BibLaTeX import support.
//!
//! Parsing is deliberately completed before repository mutation.  The parser is a
//! small recursive, token-based BibTeX parser (not a regular-expression parser):
//! it understands balanced/quoted values, concatenation, comments, preambles and
//! string macros.  Conversion and persistence are separate from parsing.

use crate::{
    model::{
        resolve_entry_type, CitationKey, Person, Reference, ReferenceType, ResolvedReferenceType,
    },
    repository::Repository,
};
use anyhow::{anyhow, bail, Context, Result};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
pub struct BibEntry {
    pub key: String,
    pub entry_type: String,
    pub fields: HashMap<String, String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DiagnosticKind {
    ExistingReference,
    DuplicateInputKey,
    InvalidCitationKey,
    InvalidMetadata,
    PersistenceFailure,
    UnsupportedType,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportDiagnostic {
    pub key: String,
    pub kind: DiagnosticKind,
    pub message: String,
}

#[derive(Clone, Debug, Default)]
pub struct ImportResult {
    pub total: usize,
    pub imported: usize,
    pub skipped: Vec<ImportDiagnostic>,
    pub failed: Vec<ImportDiagnostic>,
    pub warnings: Vec<ImportDiagnostic>,
}

impl ImportResult {
    pub fn is_complete(&self) -> bool {
        self.skipped.is_empty() && self.failed.is_empty()
    }
}

/// Parse a complete bibliography. No repository changes are made by this function.
pub fn parse_bibliography(input: &str) -> Result<Vec<BibEntry>> {
    Parser::new(input).parse()
}

/// Convert and persist parsed entries independently, without importing attachments.
pub fn import_bibliography(repo: &Repository, entries: Vec<BibEntry>) -> ImportResult {
    let mut result = ImportResult {
        total: entries.len(),
        ..ImportResult::default()
    };
    let mut seen = HashSet::new();
    for entry in entries {
        let raw_key = entry.key.clone();
        if !seen.insert(raw_key.clone()) {
            result.skipped.push(diag(
                raw_key,
                DiagnosticKind::DuplicateInputKey,
                "duplicate citation key in input",
            ));
            continue;
        }
        let key = match CitationKey::new(&raw_key) {
            Ok(key) => key,
            Err(error) => {
                result.failed.push(diag(
                    raw_key,
                    DiagnosticKind::InvalidCitationKey,
                    error.to_string(),
                ));
                continue;
            }
        };
        if repo.contains(&key) {
            result.skipped.push(diag(
                raw_key,
                DiagnosticKind::ExistingReference,
                "reference already exists",
            ));
            continue;
        }
        let (reference, warning) = match convert(&entry) {
            Ok(value) => value,
            Err(error) => {
                result.failed.push(diag(
                    raw_key,
                    DiagnosticKind::InvalidMetadata,
                    error.to_string(),
                ));
                continue;
            }
        };
        if let Some(message) = warning {
            result.warnings.push(diag(
                raw_key.clone(),
                DiagnosticKind::UnsupportedType,
                message,
            ));
        }
        // Repository::add performs normal validation, serialization, and an atomic rename.
        match repo.add(&key, &reference, None) {
            Ok(()) => result.imported += 1,
            Err(error) => result.failed.push(diag(
                raw_key,
                DiagnosticKind::PersistenceFailure,
                error.to_string(),
            )),
        }
    }
    result
}

fn diag(key: String, kind: DiagnosticKind, message: impl Into<String>) -> ImportDiagnostic {
    ImportDiagnostic {
        key,
        kind,
        message: message.into(),
    }
}

fn convert(entry: &BibEntry) -> Result<(Reference, Option<String>)> {
    let (entry_type, warning) = match resolve_entry_type(&entry.entry_type) {
        ResolvedReferenceType::Canonical(kind)
        | ResolvedReferenceType::Alias {
            canonical: kind, ..
        } => (kind, None),
        ResolvedReferenceType::Unknown(raw) => (
            ReferenceType::Misc,
            Some(format!("unsupported type `{raw}`; imported as `misc`")),
        ),
    };
    let field = |name: &str| entry.fields.get(name).map(|s| s.trim().to_owned());
    let title = field("title").context("missing title")?;
    let authors = field("author")
        .map(|value| parse_names(&value))
        .transpose()?
        .unwrap_or_default();
    let year = if let Some(value) = field("year") {
        Some(
            value
                .parse::<u16>()
                .with_context(|| format!("invalid year `{value}`"))?,
        )
    } else {
        field("date").and_then(|date| {
            let prefix = date.get(..4)?;
            (prefix.bytes().all(|c| c.is_ascii_digit())
                && (date.len() == 4 || date.as_bytes().get(4) == Some(&b'-')))
            .then(|| prefix.parse().ok())
            .flatten()
        })
    };
    let container_title = match entry_type {
        ReferenceType::Article => field("journaltitle").or_else(|| field("journal")),
        ReferenceType::Inbook | ReferenceType::Incollection | ReferenceType::Inproceedings => {
            field("booktitle")
        }
        _ => field("journaltitle").or_else(|| field("booktitle")),
    };
    let reference = Reference {
        entry_type,
        title,
        authors,
        year,
        container_title,
        publisher: field("publisher"),
        volume: field("volume"),
        issue: field("number").or_else(|| field("issue")),
        pages: field("pages").map(|p| normalize_pages(&p)),
        doi: field("doi").map(|doi| normalize_doi(&doi)),
        url: field("url"),
        tags: field("keywords")
            .map(|keywords| {
                keywords
                    .split(',')
                    .map(str::trim)
                    .filter(|tag| !tag.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        notes: field("note"),
    };
    reference.validate()?;
    Ok((reference, warning))
}

fn normalize_pages(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut previous_hyphen = false;
    for ch in value.chars() {
        if ch == '-' {
            if !previous_hyphen {
                output.push(ch);
            }
            previous_hyphen = true;
        } else {
            previous_hyphen = false;
            output.push(ch);
        }
    }
    output
}

fn normalize_doi(value: &str) -> String {
    value
        .strip_prefix("https://doi.org/")
        .or_else(|| value.strip_prefix("http://doi.org/"))
        .unwrap_or(value)
        .to_owned()
}

fn parse_names(input: &str) -> Result<Vec<Person>> {
    split_top_level(input, " and ")
        .into_iter()
        .map(|name| {
            let name = name.trim();
            if name.is_empty() {
                bail!("empty author name");
            }
            if let Some((family, given)) = split_comma(name) {
                if family.trim().is_empty() {
                    bail!("author has an empty family name");
                }
                Ok(Person {
                    given: given.trim().to_owned(),
                    family: family.trim().to_owned(),
                })
            } else {
                let words: Vec<_> = name.split_whitespace().collect();
                if words.len() == 1 {
                    Ok(Person {
                        given: String::new(),
                        family: words[0].to_owned(),
                    })
                } else {
                    Ok(Person {
                        given: words[..words.len() - 1].join(" "),
                        family: words[words.len() - 1].to_owned(),
                    })
                }
            }
        })
        .collect()
}

fn split_comma(value: &str) -> Option<(&str, &str)> {
    let mut depth = 0;
    for (index, ch) in value.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => depth -= 1,
            ',' if depth == 0 => return Some((&value[..index], &value[index + 1..])),
            _ => {}
        }
    }
    None
}

fn split_top_level<'a>(value: &'a str, separator: &str) -> Vec<&'a str> {
    let mut depth = 0;
    let mut start = 0;
    let mut parts = Vec::new();
    for (index, ch) in value.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => depth -= 1,
            _ if depth == 0 && value[index..].starts_with(separator) => {
                parts.push(&value[start..index]);
                start = index + separator.len();
            }
            _ => {}
        }
    }
    parts.push(&value[start..]);
    parts
}

struct Parser<'a> {
    source: &'a str,
    offset: usize,
    macros: HashMap<String, String>,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            offset: 0,
            macros: HashMap::new(),
        }
    }

    fn parse(mut self) -> Result<Vec<BibEntry>> {
        let mut entries = Vec::new();
        while self.skip_to_at() {
            self.bump();
            let kind = self.identifier()?;
            self.space();
            let open = self
                .bump()
                .context("expected `{` or `(` after entry type")?;
            let close = match open {
                b'{' => b'}',
                b'(' => b')',
                _ => return self.error("expected `{` or `(` after entry type"),
            };
            match kind.to_ascii_lowercase().as_str() {
                "comment" | "preamble" => self.skip_balanced(open, close)?,
                "string" => self.parse_string(close)?,
                _ => entries.push(self.parse_entry(kind, close)?),
            }
        }
        Ok(entries)
    }

    fn parse_string(&mut self, close: u8) -> Result<()> {
        self.space();
        let name = self.identifier()?.to_ascii_lowercase();
        self.space();
        self.expect(b'=')?;
        let value = self.value()?;
        self.space();
        if self.peek() == Some(b',') {
            self.bump();
            self.space();
        }
        self.expect(close)?;
        self.macros.insert(name, value);
        Ok(())
    }

    fn parse_entry(&mut self, entry_type: String, close: u8) -> Result<BibEntry> {
        self.space();
        let start = self.offset;
        while !matches!(self.peek(), Some(b',') | Some(b'}') | Some(b')') | None) {
            self.bump();
        }
        let key = self.source[start..self.offset].trim().to_owned();
        if key.is_empty() {
            return self.error("entry has an empty citation key");
        }
        self.expect(b',')?;
        let mut fields = HashMap::new();
        loop {
            self.space();
            if self.peek() == Some(close) {
                self.bump();
                break;
            }
            let name = self.identifier()?.to_ascii_lowercase();
            self.space();
            self.expect(b'=')?;
            let value = self.value()?;
            fields.insert(name, value);
            self.space();
            match self.peek() {
                Some(b',') => {
                    self.bump();
                }
                Some(byte) if byte == close => {}
                _ => return self.error("expected `,` or end of entry"),
            }
        }
        Ok(BibEntry {
            key,
            entry_type,
            fields,
        })
    }

    fn value(&mut self) -> Result<String> {
        self.space();
        let mut output = self.atom()?;
        loop {
            self.space();
            if self.peek() != Some(b'#') {
                return Ok(output);
            }
            self.bump();
            self.space();
            output.push_str(&self.atom()?);
        }
    }

    fn atom(&mut self) -> Result<String> {
        match self.peek() {
            Some(b'{') => self.braced(),
            Some(b'"') => self.quoted(),
            Some(_) => {
                let value = self.identifier()?;
                Ok(self
                    .macros
                    .get(&value.to_ascii_lowercase())
                    .cloned()
                    .unwrap_or(value))
            }
            None => self.error("unexpected end of file in value"),
        }
    }

    fn braced(&mut self) -> Result<String> {
        self.expect(b'{')?;
        let mut depth = 1;
        let mut output = Vec::new();
        while let Some(byte) = self.bump() {
            match byte {
                b'\\' => {
                    output.push(b'\\');
                    if let Some(next) = self.bump() {
                        output.push(next);
                    }
                }
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return String::from_utf8(output)
                            .context("bibliography value is not UTF-8");
                    }
                }
                _ => output.push(byte),
            }
        }
        self.error("unexpected end of file in braced value")
    }

    fn quoted(&mut self) -> Result<String> {
        self.expect(b'"')?;
        let mut output = Vec::new();
        let mut depth = 0;
        while let Some(byte) = self.bump() {
            match byte {
                b'\\' => {
                    output.push(b'\\');
                    if let Some(next) = self.bump() {
                        output.push(next);
                    }
                }
                b'{' => depth += 1,
                b'}' if depth > 0 => depth -= 1,
                b'"' if depth == 0 => {
                    return String::from_utf8(output).context("bibliography value is not UTF-8")
                }
                _ => output.push(byte),
            }
        }
        self.error("unexpected end of file in quoted value")
    }

    fn skip_balanced(&mut self, open: u8, close: u8) -> Result<()> {
        let mut depth = 1;
        while let Some(byte) = self.bump() {
            if byte == open {
                depth += 1;
            } else if byte == close {
                depth -= 1;
                if depth == 0 {
                    return Ok(());
                }
            }
        }
        self.error("unexpected end of file in directive")
    }

    fn identifier(&mut self) -> Result<String> {
        self.space();
        let start = self.offset;
        while self.peek().is_some_and(|byte| {
            !byte.is_ascii_whitespace()
                && !matches!(byte, b'=' | b',' | b'{' | b'}' | b'(' | b')' | b'#')
        }) {
            self.bump();
        }
        if start == self.offset {
            self.error("expected identifier")
        } else {
            Ok(self.source[start..self.offset].to_owned())
        }
    }

    fn skip_to_at(&mut self) -> bool {
        while let Some(byte) = self.peek() {
            if byte == b'@' {
                return true;
            }
            self.bump();
        }
        false
    }

    fn space(&mut self) {
        loop {
            while self.peek().is_some_and(|b| b.is_ascii_whitespace()) {
                self.bump();
            }
            if self.peek() == Some(b'%') {
                while self.peek().is_some_and(|b| b != b'\n') {
                    self.bump();
                }
            } else {
                break;
            }
        }
    }

    fn expect(&mut self, expected: u8) -> Result<()> {
        self.space();
        if self.bump() == Some(expected) {
            Ok(())
        } else {
            self.error(&format!("expected `{}`", expected as char))
        }
    }

    fn peek(&self) -> Option<u8> {
        self.source.as_bytes().get(self.offset).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let value = self.peek()?;
        self.offset += 1;
        Some(value)
    }

    fn error<T>(&self, message: &str) -> Result<T> {
        let before = &self.source[..self.offset.min(self.source.len())];
        let line = before.bytes().filter(|b| *b == b'\n').count() + 1;
        let column = before
            .rsplit_once('\n')
            .map_or(before.len() + 1, |(_, tail)| tail.len() + 1);
        Err(anyhow!("{message} at line {line}, column {column}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Scenario: parser expands macros and balances title braces.
    // Requirements: REQ-048, REQ-049
    #[test]
    fn parser_expands_macros_and_balances_title_braces() {
        let entries = parse_bibliography(
            r#"@string{j = "Journal"}
               @comment{ignored}
               @article{x, title={Using {EEG}}, author={Smith, Jane}, journal=j}"#,
        )
        .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].fields["title"], "Using EEG");
        assert_eq!(entries[0].fields["journal"], "Journal");
    }

    // Scenario: malformed input has location.
    // Requirement: REQ-040
    #[test]
    fn malformed_input_has_location() {
        let error = parse_bibliography("@article{x, title={oops}").unwrap_err();
        assert!(error.to_string().contains("line 1, column"));
    }
}
