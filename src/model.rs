use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct CitationKey(String);

impl CitationKey {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        let valid = !matches!(value.as_str(), "" | "." | "..")
            && value.as_bytes()[0].is_ascii_alphanumeric()
            && value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'));
        if !valid {
            bail!("invalid citation key `{value}` (expected [A-Za-z0-9][A-Za-z0-9._-]*)");
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CitationKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ReferenceType {
    Article,
    Book,
    Inbook,
    Incollection,
    Inproceedings,
    Proceedings,
    Thesis,
    Report,
    Misc,
    Online,
}

impl fmt::Display for ReferenceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = serde_yaml::to_string(self).map_err(|_| fmt::Error)?;
        f.write_str(s.trim())
    }
}

impl FromStr for ReferenceType {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        serde_yaml::from_str(s).map_err(|_| anyhow::anyhow!("unknown reference type `{s}`"))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Person {
    pub given: String,
    pub family: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    #[serde(rename = "type")]
    pub entry_type: ReferenceType,
    pub title: String,
    pub authors: Vec<Person>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issue: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pages: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doi: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

impl Reference {
    pub fn validate(&self) -> Result<()> {
        if self.title.trim().is_empty() {
            bail!("title is empty");
        }
        for (i, author) in self.authors.iter().enumerate() {
            if author.family.trim().is_empty() {
                bail!("author {} has an empty family name", i + 1);
            }
        }
        if let Some(year) = self.year {
            validate_year(year)?;
        }
        Ok(())
    }
}

/// Validate a publication year consistently at every input boundary.
pub fn validate_year(year: u16) -> Result<()> {
    if !(1000..=3000).contains(&year) {
        bail!("year `{year}` is not plausible");
    }
    Ok(())
}

pub fn generated_key(family: &str, year: Option<u16>) -> String {
    let mut key: String = family
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    if key.is_empty() {
        key.push_str("reference");
    }
    if let Some(year) = year {
        key.push_str(&year.to_string());
    }
    key
}

pub fn display_author(authors: &[Person]) -> String {
    match authors {
        [] => "-".into(),
        [a] => a.family.clone(),
        [a, b] => format!("{} & {}", a.family, b.family),
        [a, ..] => format!("{} et al.", a.family),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keys() {
        for k in ["foo", "A.b-c_2"] {
            assert!(CitationKey::new(k).is_ok());
        }
        for k in ["", ".", "..", "a/b", "two words"] {
            assert!(CitationKey::new(k).is_err());
        }
    }
    #[test]
    fn generation() {
        assert_eq!(generated_key("Rocchio", Some(1971)), "rocchio1971");
        assert_eq!(generated_key("García-López", None), "garcalpez");
    }
    #[test]
    fn unknown_type() {
        assert!("journall".parse::<ReferenceType>().is_err());
    }
}
