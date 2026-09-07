//! Conservative, read-only analysis for `ref clean`.

use crate::{
    model::CitationKey,
    repository::{Repository, StoredReference},
};
use anyhow::{bail, Context, Result};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug)]
pub struct ScanError {
    pub path: PathBuf,
    pub message: String,
}

#[derive(Clone, Debug)]
pub struct CleanAnalysis {
    pub repository_root: PathBuf,
    pub scan_root: PathBuf,
    pub files_scanned: usize,
    pub used: Vec<StoredReference>,
    pub unused: Vec<StoredReference>,
    pub scan_errors: Vec<ScanError>,
}

/// Analyze usage below `scan_root`. This function never mutates the repository.
pub fn analyze(repo: &Repository, scan_root: &Path, paths: &[PathBuf]) -> Result<CleanAnalysis> {
    let references = repo.load_all()?;
    let scan_root = scan_root
        .canonicalize()
        .context("failed to resolve clean scan root")?;
    let scopes = resolve_scopes(&scan_root, paths)?;
    let mut used = BTreeSet::new();
    let mut seen_files = BTreeSet::new();
    let mut errors = Vec::new();
    let mut files_scanned = 0;

    let project_root = repo.root().parent().unwrap_or(repo.root());
    let ignores = IgnoreRules::load(project_root);
    for scope in scopes {
        let mut candidates = Vec::new();
        collect_files(
            &scope,
            &scan_root,
            repo.root(),
            project_root,
            &ignores,
            &mut candidates,
            &mut errors,
        );
        for path in candidates {
            if !seen_files.insert(path.to_path_buf()) {
                continue;
            }
            let bytes = match fs::read(&path) {
                Ok(bytes) => bytes,
                Err(error) => {
                    errors.push(ScanError {
                        path: path.clone(),
                        message: error.to_string(),
                    });
                    continue;
                }
            };
            if bytes.contains(&0) {
                continue;
            }
            let text = match std::str::from_utf8(&bytes) {
                Ok(text) => text,
                Err(error) => {
                    errors.push(ScanError {
                        path: path.clone(),
                        message: format!("invalid UTF-8: {error}"),
                    });
                    continue;
                }
            };
            files_scanned += 1;
            for reference in &references {
                if contains_key(text.as_bytes(), reference.key.as_str().as_bytes()) {
                    used.insert(reference.key.clone());
                }
            }
        }
    }
    let (used_refs, unused): (Vec<_>, Vec<_>) =
        references.into_iter().partition(|r| used.contains(&r.key));
    Ok(CleanAnalysis {
        repository_root: repo.root().to_path_buf(),
        scan_root,
        files_scanned,
        used: used_refs,
        unused,
        scan_errors: errors,
    })
}

#[derive(Default)]
struct IgnoreRules(Vec<(String, bool)>);

impl IgnoreRules {
    fn load(project_root: &Path) -> Self {
        let mut rules = Vec::new();
        for file in [
            project_root.join(".gitignore"),
            project_root.join(".git/info/exclude"),
        ] {
            if let Ok(text) = fs::read_to_string(file) {
                rules.extend(
                    text.lines()
                        .map(str::trim)
                        .filter(|line| !line.is_empty() && !line.starts_with('#'))
                        .map(|line| {
                            let (pattern, ignored) = line
                                .strip_prefix('!')
                                .map_or((line, true), |pattern| (pattern, false));
                            (pattern.trim_matches('/').to_owned(), ignored)
                        }),
                );
            }
        }
        Self(rules)
    }
    fn matches(&self, path: &Path, project_root: &Path) -> bool {
        let relative = path
            .strip_prefix(project_root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        self.0.iter().fold(false, |ignored, (rule, rule_ignored)| {
            let rule = rule.trim_end_matches('/');
            if let Some(suffix) = rule.strip_prefix("*.") {
                return if path.extension().is_some_and(|ext| ext == suffix) {
                    *rule_ignored
                } else {
                    ignored
                };
            }
            let matches = relative == rule
                || relative.starts_with(&format!("{rule}/"))
                || (!rule.contains('/') && path.components().any(|c| c.as_os_str() == rule));
            if matches {
                *rule_ignored
            } else {
                ignored
            }
        })
    }
}

fn collect_files(
    path: &Path,
    scan_root: &Path,
    repository: &Path,
    project_root: &Path,
    ignores: &IgnoreRules,
    files: &mut Vec<PathBuf>,
    errors: &mut Vec<ScanError>,
) {
    if explicitly_excluded(path, scan_root, repository) || ignores.matches(path, project_root) {
        return;
    }
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => {
            errors.push(ScanError {
                path: path.to_path_buf(),
                message: error.to_string(),
            });
            return;
        }
    };
    if metadata.file_type().is_symlink() {
        return;
    }
    if metadata.is_file() {
        files.push(path.to_path_buf());
        return;
    }
    if !metadata.is_dir() {
        return;
    }
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) => {
            errors.push(ScanError {
                path: path.to_path_buf(),
                message: error.to_string(),
            });
            return;
        }
    };
    for entry in entries {
        match entry {
            Ok(entry) => collect_files(
                &entry.path(),
                scan_root,
                repository,
                project_root,
                ignores,
                files,
                errors,
            ),
            Err(error) => errors.push(ScanError {
                path: path.to_path_buf(),
                message: error.to_string(),
            }),
        }
    }
}

fn resolve_scopes(scan_root: &Path, paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    if paths.is_empty() {
        return Ok(vec![scan_root.to_path_buf()]);
    }
    paths
        .iter()
        .map(|path| {
            let joined = if path.is_absolute() {
                path.clone()
            } else {
                scan_root.join(path)
            };
            let resolved = joined
                .canonicalize()
                .with_context(|| format!("clean path `{}` does not exist", path.display()))?;
            if !resolved.starts_with(scan_root) {
                bail!("clean paths must remain within the current directory");
            }
            Ok(resolved)
        })
        .collect()
}

fn explicitly_excluded(path: &Path, scan_root: &Path, repository: &Path) -> bool {
    if path.starts_with(repository) {
        return true;
    }
    let relative = path.strip_prefix(scan_root).unwrap_or(path);
    if relative
        .components()
        .any(|part| matches!(part.as_os_str().to_str(), Some(".git") | Some(".ref")))
    {
        return true;
    }
    path.extension()
        .and_then(|x| x.to_str())
        .is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "bib" | "bibtex" | "pdf"))
}

fn contains_key(text: &[u8], key: &[u8]) -> bool {
    text.windows(key.len())
        .enumerate()
        .any(|(index, candidate)| {
            candidate == key
                && index
                    .checked_sub(1)
                    .is_none_or(|before| !CitationKey::is_valid_byte(text[before]))
                && text
                    .get(index + key.len())
                    .is_none_or(|after| !CitationKey::is_valid_byte(*after))
        })
}

#[cfg(test)]
mod tests {
    use super::contains_key;
    // Scenario: key matches only at identity boundaries.
    // Requirement: REQ-031
    #[test]
    fn key_matches_only_at_identity_boundaries() {
        assert!(contains_key(
            b"\\cite{Smith2024Attention}",
            b"Smith2024Attention"
        ));
        assert!(!contains_key(
            b"Smith2024AttentionModel",
            b"Smith2024Attention"
        ));
        assert!(!contains_key(
            b"x-Smith2024Attention",
            b"Smith2024Attention"
        ));
    }
}
