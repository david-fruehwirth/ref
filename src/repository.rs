use crate::model::{CitationKey, Reference};
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Deserialize, Serialize)]
struct Config {
    version: u8,
    #[serde(default = "default_pdf_directory")]
    pdf_directory: PathBuf,
}

fn default_pdf_directory() -> PathBuf {
    PathBuf::from("source")
}

#[derive(Clone, Debug)]
pub struct StoredReference {
    pub key: CitationKey,
    pub metadata: Reference,
    pub path: PathBuf,
    pub has_pdf: bool,
    pub pdf_filename: Option<String>,
    pub pdf_path: Option<PathBuf>,
    /// Operational creation metadata; absent for repositories created by older versions.
    pub added_at: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Repository {
    root: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceProofStatus {
    Present(PathBuf),
    Missing,
    Invalid(String),
}

#[derive(Clone, Debug)]
pub struct PdfMigration {
    pub key: CitationKey,
    pub from: PathBuf,
    pub to: PathBuf,
}

impl Repository {
    pub fn init(project: &Path) -> Result<Self> {
        let root = project.join(".ref");
        if root.exists() {
            bail!("reference repository already exists at {}", root.display());
        }
        let tmp = tempfile::Builder::new()
            .prefix(".ref-init-")
            .tempdir_in(project)?;
        fs::create_dir(tmp.path().join("refs"))?;
        fs::write(
            tmp.path().join("config.yaml"),
            "version: 1\npdf_directory: source\n",
        )?;
        let temp_path = tmp.keep();
        fs::rename(&temp_path, &root)
            .with_context(|| format!("failed to initialize {}", root.display()))?;
        Ok(Self { root })
    }

    pub fn discover(start: &Path) -> Result<Self> {
        for dir in start.ancestors() {
            let root = dir.join(".ref");
            if root.is_dir() {
                return Ok(Self { root });
            }
        }
        bail!("not inside a ref repository\nhint: run `ref init` to create one")
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn references_dir(&self) -> PathBuf {
        self.root.join("refs")
    }
    pub fn reference_path(&self, key: &CitationKey) -> PathBuf {
        self.references_dir().join(key.as_str())
    }
    pub fn contains(&self, key: &CitationKey) -> bool {
        self.reference_path(key).is_dir()
    }

    fn config(&self) -> Result<Config> {
        let path = self.root.join("config.yaml");
        serde_yaml::from_str(
            &fs::read_to_string(&path).with_context(|| format!("missing {}", path.display()))?,
        )
        .with_context(|| format!("invalid {}", path.display()))
    }

    pub fn pdf_directory(&self) -> Result<PathBuf> {
        let configured = self.config()?.pdf_directory;
        let path = if configured.is_absolute() {
            configured
        } else {
            self.root.join(configured)
        };
        if path.exists() && !path.is_dir() {
            bail!("configured PDF directory `{}` exists but is not a directory\nhint: update `pdf_directory` in {}", path.display(), self.root.join("config.yaml").display());
        }
        Ok(path)
    }

    pub fn pdf_path(&self, key: &CitationKey, filename: &str) -> Result<PathBuf> {
        let expected = format!("{key}.pdf");
        if filename != expected || Path::new(filename).file_name() != Some(filename.as_ref()) {
            bail!(
                "invalid pdf_filename `{filename}` for reference `{key}` (expected `{expected}`)"
            );
        }
        Ok(self.pdf_directory()?.join(filename))
    }

    /// Inspect the canonical source artifact. `metadata` follows symlinks, which
    /// keeps this policy consistent with opening an attachment via `is_file`.
    pub fn source_proof_status(&self, key: &CitationKey) -> SourceProofStatus {
        let loaded = match self.load_reference(key) {
            Ok(reference) => reference,
            Err(error) => return SourceProofStatus::Invalid(error.to_string()),
        };
        let Some(path) = loaded.pdf_path else {
            return SourceProofStatus::Missing;
        };
        inspect_pdf(path)
    }

    pub fn source_pdf_path(&self, key: &CitationKey) -> Result<PathBuf> {
        match self.source_proof_status(key) {
            SourceProofStatus::Present(path) => Ok(path),
            SourceProofStatus::Missing => bail!("reference `{key}` has no PDF"),
            SourceProofStatus::Invalid(reason) => {
                bail!("reference `{key}` has no available PDF: {reason}")
            }
        }
    }

    pub fn attach(&self, key: &CitationKey, source: &Path) -> Result<()> {
        if !self.contains(key) {
            bail!("reference `{key}` does not exist");
        }
        if !source.is_file() {
            bail!(
                "PDF `{}` does not exist or is not a regular file",
                source.display()
            );
        }
        if !source
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
        {
            bail!("file must have a .pdf extension");
        }
        if fs::metadata(source)?.len() == 0 {
            bail!("PDF `{}` is empty", source.display());
        }
        let mut reference = self.load_reference(key)?;
        if reference.pdf_filename.is_some() {
            bail!("reference `{key}` already has a source PDF");
        }
        let directory = self.pdf_directory()?;
        fs::create_dir_all(&directory).with_context(|| {
            format!(
                "failed to create configured PDF directory {}",
                directory.display()
            )
        })?;
        let filename = format!("{key}.pdf");
        let destination = directory.join(&filename);
        if destination.exists() {
            bail!(
                "PDF destination `{}` already exists; refusing to overwrite it",
                destination.display()
            );
        }
        let mut temporary = tempfile::NamedTempFile::new_in(&directory)?;
        let mut input = fs::File::open(source)?;
        std::io::copy(&mut input, &mut temporary)?;
        temporary
            .persist_noclobber(&destination)
            .map_err(|error| error.error)
            .with_context(|| format!("failed to attach source PDF to `{key}`"))?;
        reference.pdf_filename = Some(filename);
        if let Err(error) = self.write_stored_metadata(&reference) {
            let _ = fs::remove_file(&destination);
            return Err(error);
        }
        Ok(())
    }

    pub fn validate_structure(&self) -> Result<()> {
        let config = self.config()?;
        if config.version != 1 {
            bail!("unsupported repository version {}", config.version);
        }
        if !self.references_dir().is_dir() {
            bail!("missing {}", self.references_dir().display());
        }
        self.pdf_directory()?;
        Ok(())
    }

    pub fn load_reference(&self, key: &CitationKey) -> Result<StoredReference> {
        let path = self.reference_path(key);
        if !path.is_dir() {
            bail!("reference `{key}` does not exist");
        }
        let yaml = path.join("ref.yaml");
        let contents = fs::read_to_string(&yaml)
            .with_context(|| format!("failed to read {}", yaml.display()))?;
        let (metadata, added_at, pdf_filename) = parse_reference_yaml(&contents)
            .with_context(|| format!("failed to parse {}", yaml.display()))?;
        metadata
            .validate()
            .with_context(|| format!("invalid metadata in {}", yaml.display()))?;
        if pdf_filename.is_none() && path.join("paper.pdf").exists() {
            bail!("legacy per-reference PDF found for `{key}`; run `ref migrate-pdfs --dry-run`, then `ref migrate-pdfs`");
        }
        let pdf_path = pdf_filename
            .as_deref()
            .map(|name| self.pdf_path(key, name))
            .transpose()?;
        let has_pdf = pdf_path
            .as_ref()
            .is_some_and(|path| matches!(inspect_pdf(path.clone()), SourceProofStatus::Present(_)));
        Ok(StoredReference {
            key: key.clone(),
            metadata,
            has_pdf,
            pdf_filename,
            pdf_path,
            path,
            added_at,
        })
    }

    /// Return references with reliable persisted creation metadata, newest first.
    pub fn recent_references(&self, limit: usize) -> Result<Vec<StoredReference>> {
        let mut references: Vec<_> = self
            .load_all()?
            .into_iter()
            .filter(|reference| reference.added_at.is_some())
            .collect();
        references.sort_by(|a, b| b.added_at.cmp(&a.added_at).then_with(|| a.key.cmp(&b.key)));
        references.truncate(limit);
        Ok(references)
    }

    pub fn load_all(&self) -> Result<Vec<StoredReference>> {
        self.validate_structure()?;
        let mut entries = Vec::new();
        for entry in fs::read_dir(self.references_dir())? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| anyhow!("non-UTF-8 reference directory"))?;
            let key = CitationKey::new(name)?;
            entries.push(self.load_reference(&key)?);
        }
        entries.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(entries)
    }

    pub fn add(&self, key: &CitationKey, metadata: &Reference, pdf: Option<&Path>) -> Result<()> {
        metadata.validate()?;
        if self.contains(key) {
            bail!("reference `{key}` already exists");
        }
        let tmp = tempfile::Builder::new()
            .prefix(".ref-add-")
            .tempdir_in(self.references_dir())?;
        let mut value = serde_yaml::to_value(metadata)?;
        value
            .as_mapping_mut()
            .context("reference metadata did not serialize as a mapping")?
            .insert(
                serde_yaml::Value::String("added_at".into()),
                serde_yaml::Value::String(utc_now()?),
            );
        let filename = pdf.map(|_| format!("{key}.pdf"));
        if let Some(filename) = &filename {
            value.as_mapping_mut().unwrap().insert(
                serde_yaml::Value::String("pdf_filename".into()),
                serde_yaml::Value::String(filename.clone()),
            );
        }
        fs::write(tmp.path().join("ref.yaml"), serde_yaml::to_string(&value)?)?;
        let mut committed_pdf = None;
        if let Some(pdf) = pdf {
            let directory = self.pdf_directory()?;
            fs::create_dir_all(&directory)?;
            let destination = directory.join(filename.as_ref().unwrap());
            if destination.exists() {
                bail!(
                    "PDF destination `{}` already exists; refusing to overwrite it",
                    destination.display()
                );
            }
            let mut staged = tempfile::NamedTempFile::new_in(&directory)?;
            std::io::copy(&mut fs::File::open(pdf)?, &mut staged)?;
            staged
                .persist_noclobber(&destination)
                .map_err(|e| e.error)?;
            committed_pdf = Some(destination);
        }
        let temp_path = tmp.keep();
        if let Err(error) = fs::rename(&temp_path, self.reference_path(key)) {
            // `TempDir::keep` transfers cleanup responsibility to us. Never leave
            // an import/add staging directory behind after a failed atomic commit.
            let _ = fs::remove_dir_all(&temp_path);
            if let Some(pdf) = committed_pdf {
                let _ = fs::remove_file(pdf);
            }
            return Err(error).context("failed to commit reference");
        }
        Ok(())
    }

    pub fn rename(&self, old: &CitationKey, new: &CitationKey) -> Result<()> {
        if !self.contains(old) {
            bail!("reference `{old}` does not exist");
        }
        if self.contains(new) {
            bail!("reference `{new}` already exists");
        }
        let mut reference = self.load_reference(old)?;
        let old_pdf = reference.pdf_path.clone();
        let new_pdf = old_pdf
            .as_ref()
            .map(|_| self.pdf_directory().map(|d| d.join(format!("{new}.pdf"))))
            .transpose()?;
        if new_pdf.as_ref().is_some_and(|p| p.exists()) {
            bail!(
                "PDF destination `{}` already exists; refusing to overwrite it",
                new_pdf.as_ref().unwrap().display()
            );
        }
        fs::rename(self.reference_path(old), self.reference_path(new))?;
        reference.key = new.clone();
        if let (Some(from), Some(to)) = (&old_pdf, &new_pdf) {
            if let Err(error) = fs::rename(from, to) {
                let _ = fs::rename(self.reference_path(new), self.reference_path(old));
                return Err(error).context("failed to rename source PDF");
            }
            reference.pdf_filename = Some(format!("{new}.pdf"));
            reference.pdf_path = Some(to.clone());
            if let Err(error) = self.write_stored_metadata(&reference) {
                let _ = fs::rename(to, from);
                let _ = fs::rename(self.reference_path(new), self.reference_path(old));
                return Err(error);
            }
        }
        Ok(())
    }

    pub fn remove(&self, key: &CitationKey) -> Result<()> {
        if !self.contains(key) {
            bail!("reference `{key}` does not exist");
        }
        let reference = self.load_reference(key)?;
        if let Some(pdf) = reference.pdf_path {
            if pdf.exists() {
                fs::remove_file(&pdf)
                    .with_context(|| format!("failed to remove source PDF {}", pdf.display()))?;
            }
        }
        fs::remove_dir_all(self.reference_path(key))?;
        Ok(())
    }

    /// Plan or perform the explicit migration from the legacy per-reference layout.
    pub fn migrate_pdfs(&self, dry_run: bool) -> Result<Vec<PdfMigration>> {
        self.validate_structure()?;
        let directory = self.pdf_directory()?;
        let mut changes = Vec::new();
        for entry in fs::read_dir(self.references_dir())? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let key = CitationKey::new(
                entry
                    .file_name()
                    .into_string()
                    .map_err(|_| anyhow!("non-UTF-8 reference directory"))?,
            )?;
            let from = entry.path().join("paper.pdf");
            if !from.exists() {
                continue;
            }
            if !from.is_file() {
                bail!("legacy PDF `{}` is not a regular file", from.display());
            }
            let to = directory.join(format!("{key}.pdf"));
            if to.exists() {
                bail!(
                    "migration destination `{}` already exists; no files were changed",
                    to.display()
                );
            }
            let text = fs::read_to_string(entry.path().join("ref.yaml"))?;
            let (_, _, filename) = parse_reference_yaml(&text)?;
            if filename.is_some() {
                bail!("reference `{key}` has both legacy PDF storage and pdf_filename; resolve this conflict manually");
            }
            changes.push(PdfMigration { key, from, to });
        }
        changes.sort_by(|a, b| a.key.cmp(&b.key));
        if dry_run || changes.is_empty() {
            return Ok(changes);
        }
        fs::create_dir_all(&directory)?;
        for change in &changes {
            fs::rename(&change.from, &change.to).with_context(|| {
                format!(
                    "failed to move `{}`; the original PDF was not deleted",
                    change.from.display()
                )
            })?;
            let yaml = self.reference_path(&change.key).join("ref.yaml");
            let text = fs::read_to_string(&yaml)?;
            let (metadata, added_at, _) = parse_reference_yaml(&text)?;
            let stored = StoredReference {
                key: change.key.clone(),
                metadata,
                path: self.reference_path(&change.key),
                has_pdf: true,
                pdf_filename: Some(format!("{}.pdf", change.key)),
                pdf_path: Some(change.to.clone()),
                added_at,
            };
            if let Err(error) = self.write_stored_metadata(&stored) {
                let _ = fs::rename(&change.to, &change.from);
                return Err(error).context(format!(
                    "failed to update metadata for `{}`; its PDF was restored",
                    change.key
                ));
            }
        }
        Ok(changes)
    }

    fn write_stored_metadata(&self, reference: &StoredReference) -> Result<()> {
        let mut value = serde_yaml::to_value(&reference.metadata)?;
        let map = value
            .as_mapping_mut()
            .context("reference metadata did not serialize as a mapping")?;
        if let Some(added) = &reference.added_at {
            map.insert("added_at".into(), added.clone().into());
        }
        if let Some(filename) = &reference.pdf_filename {
            map.insert("pdf_filename".into(), filename.clone().into());
        }
        let path = self.reference_path(&reference.key).join("ref.yaml");
        let mut temporary = tempfile::NamedTempFile::new_in(self.reference_path(&reference.key))?;
        use std::io::Write;
        temporary.write_all(serde_yaml::to_string(&value)?.as_bytes())?;
        temporary.persist(&path).map_err(|e| e.error)?;
        Ok(())
    }
}

fn inspect_pdf(path: PathBuf) -> SourceProofStatus {
    match fs::metadata(&path) {
        Ok(metadata) if !metadata.is_file() => {
            SourceProofStatus::Invalid("expected a regular file".into())
        }
        Ok(metadata) if metadata.len() == 0 => SourceProofStatus::Invalid("file is empty".into()),
        Ok(_) => match fs::File::open(&path) {
            Ok(_) => SourceProofStatus::Present(path),
            Err(error) => SourceProofStatus::Invalid(format!("cannot read file: {error}")),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => SourceProofStatus::Missing,
        Err(error) => SourceProofStatus::Invalid(format!("cannot inspect file: {error}")),
    }
}

pub(crate) fn parse_reference_yaml(
    contents: &str,
) -> Result<(Reference, Option<String>, Option<String>)> {
    let mut value: serde_yaml::Value = serde_yaml::from_str(contents)?;
    let added_at = value
        .as_mapping_mut()
        .and_then(|mapping| mapping.remove(serde_yaml::Value::String("added_at".into())))
        .map(serde_yaml::from_value::<String>)
        .transpose()?;
    if let Some(timestamp) = &added_at {
        validate_utc_timestamp(timestamp)?;
    }
    let pdf_filename = value
        .as_mapping_mut()
        .and_then(|mapping| mapping.remove(serde_yaml::Value::String("pdf_filename".into())))
        .map(serde_yaml::from_value::<String>)
        .transpose()?;
    Ok((serde_yaml::from_value(value)?, added_at, pdf_filename))
}

fn validate_utc_timestamp(value: &str) -> Result<()> {
    let bytes = value.as_bytes();
    let shape = bytes.len() == 30
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':'
        && bytes[19] == b'.'
        && bytes[29] == b'Z'
        && bytes.iter().enumerate().all(|(index, byte)| {
            matches!(index, 4 | 7 | 10 | 13 | 16 | 19 | 29) || byte.is_ascii_digit()
        });
    if !shape {
        bail!("expected a UTC timestamp in YYYY-MM-DDTHH:MM:SS.nnnnnnnnnZ form");
    }
    let component = |range: std::ops::Range<usize>| -> Result<u32> {
        value[range]
            .parse()
            .context("timestamp contains an invalid numeric component")
    };
    let year = component(0..4)?;
    let month = component(5..7)?;
    let day = component(8..10)?;
    let hour = component(11..13)?;
    let minute = component(14..16)?;
    let second = component(17..19)?;
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let max_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    };
    if day == 0 || day > max_day || hour > 23 || minute > 59 || second > 59 {
        bail!("timestamp contains an out-of-range UTC date or time");
    }
    Ok(())
}

/// Format `SystemTime` without relying on platform-specific time APIs.
fn utc_now() -> Result<String> {
    let elapsed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .context("system clock is before the Unix epoch")?;
    let seconds = elapsed.as_secs();
    let days = (seconds / 86_400) as i64;
    let day_seconds = seconds % 86_400;
    // Howard Hinnant's civil-from-days conversion (days since 1970-01-01).
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    Ok(format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:09}Z",
        day_seconds / 3_600,
        day_seconds / 60 % 60,
        day_seconds % 60,
        elapsed.subsec_nanos()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    // Scenario: init and discover.
    // Requirements: REQ-001, REQ-002
    #[test]
    fn init_and_discover() {
        let t = tempfile::tempdir().unwrap();
        let r = Repository::init(t.path()).unwrap();
        let nested = t.path().join("a/b");
        fs::create_dir_all(&nested).unwrap();
        assert_eq!(Repository::discover(&nested).unwrap().root(), r.root());
        assert!(Repository::init(t.path()).is_err());
    }
    // Scenario: discovery fails.
    // Requirement: REQ-003
    #[test]
    fn discovery_fails() {
        let t = tempfile::tempdir().unwrap();
        assert!(Repository::discover(t.path()).is_err());
    }

    // Scenario: failed commit removes staging directory.
    // Requirement: REQ-007
    #[test]
    fn failed_commit_removes_staging_directory() {
        let t = tempfile::tempdir().unwrap();
        let repo = Repository::init(t.path()).unwrap();
        let key = CitationKey::new("blocked").unwrap();
        fs::write(repo.reference_path(&key), "not a directory").unwrap();
        let metadata = Reference {
            entry_type: crate::model::ReferenceType::Misc,
            title: "Example".into(),
            authors: vec![],
            year: None,
            container_title: None,
            publisher: None,
            volume: None,
            issue: None,
            pages: None,
            doi: None,
            url: None,
            tags: vec![],
            notes: None,
        };
        assert!(repo.add(&key, &metadata, None).is_err());
        assert!(fs::read_dir(repo.references_dir())
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".ref-add-")));
    }
}
