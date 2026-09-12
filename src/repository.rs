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
}

#[derive(Clone, Debug)]
pub struct StoredReference {
    pub key: CitationKey,
    pub metadata: Reference,
    pub path: PathBuf,
    pub has_pdf: bool,
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
        fs::write(tmp.path().join("config.yaml"), "version: 1\n")?;
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

    /// Inspect the canonical source artifact. `metadata` follows symlinks, which
    /// keeps this policy consistent with opening an attachment via `is_file`.
    pub fn source_proof_status(&self, key: &CitationKey) -> SourceProofStatus {
        let path = self.reference_path(key).join("paper.pdf");
        match fs::metadata(&path) {
            Ok(metadata) if !metadata.is_file() => {
                SourceProofStatus::Invalid("expected a regular file".into())
            }
            Ok(metadata) if metadata.len() == 0 => {
                SourceProofStatus::Invalid("file is empty".into())
            }
            Ok(_) => match fs::File::open(&path) {
                Ok(_) => SourceProofStatus::Present(path),
                Err(error) => SourceProofStatus::Invalid(format!("cannot read file: {error}")),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                SourceProofStatus::Missing
            }
            Err(error) => SourceProofStatus::Invalid(format!("cannot inspect file: {error}")),
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
        let destination = self.reference_path(key).join("paper.pdf");
        if destination.exists() {
            bail!("reference `{key}` already has a source PDF");
        }
        let mut temporary = tempfile::NamedTempFile::new_in(self.reference_path(key))?;
        let mut input = fs::File::open(source)?;
        std::io::copy(&mut input, &mut temporary)?;
        temporary
            .persist_noclobber(&destination)
            .map_err(|error| error.error)
            .with_context(|| format!("failed to attach source PDF to `{key}`"))?;
        Ok(())
    }

    pub fn validate_structure(&self) -> Result<()> {
        let config_path = self.root.join("config.yaml");
        let config: Config = serde_yaml::from_str(
            &fs::read_to_string(&config_path)
                .with_context(|| format!("missing {}", config_path.display()))?,
        )
        .with_context(|| format!("invalid {}", config_path.display()))?;
        if config.version != 1 {
            bail!("unsupported repository version {}", config.version);
        }
        if !self.references_dir().is_dir() {
            bail!("missing {}", self.references_dir().display());
        }
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
        let (metadata, added_at) = parse_reference_yaml(&contents)
            .with_context(|| format!("failed to parse {}", yaml.display()))?;
        metadata
            .validate()
            .with_context(|| format!("invalid metadata in {}", yaml.display()))?;
        Ok(StoredReference {
            key: key.clone(),
            metadata,
            has_pdf: path.join("paper.pdf").is_file(),
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
        fs::write(tmp.path().join("ref.yaml"), serde_yaml::to_string(&value)?)?;
        if let Some(pdf) = pdf {
            fs::copy(pdf, tmp.path().join("paper.pdf"))?;
        }
        let temp_path = tmp.keep();
        if let Err(error) = fs::rename(&temp_path, self.reference_path(key)) {
            // `TempDir::keep` transfers cleanup responsibility to us. Never leave
            // an import/add staging directory behind after a failed atomic commit.
            let _ = fs::remove_dir_all(&temp_path);
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
        fs::rename(self.reference_path(old), self.reference_path(new))?;
        Ok(())
    }

    pub fn remove(&self, key: &CitationKey) -> Result<()> {
        if !self.contains(key) {
            bail!("reference `{key}` does not exist");
        }
        fs::remove_dir_all(self.reference_path(key))?;
        Ok(())
    }
}

pub(crate) fn parse_reference_yaml(contents: &str) -> Result<(Reference, Option<String>)> {
    let mut value: serde_yaml::Value = serde_yaml::from_str(contents)?;
    let added_at = value
        .as_mapping_mut()
        .and_then(|mapping| mapping.remove(serde_yaml::Value::String("added_at".into())))
        .map(serde_yaml::from_value::<String>)
        .transpose()?;
    if let Some(timestamp) = &added_at {
        validate_utc_timestamp(timestamp)?;
    }
    Ok((serde_yaml::from_value(value)?, added_at))
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
