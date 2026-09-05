use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct CitationKey(String);

impl CitationKey {
    /// Whether a byte may occur within a citation key.
    pub fn is_valid_byte(byte: u8) -> bool {
        byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
    }

    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        let valid = !matches!(value.as_str(), "" | "." | "..")
            && value.as_bytes()[0].is_ascii_alphanumeric()
            && value.bytes().all(Self::is_valid_byte);
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

/// Generate the creation-time default identity for a reference.
///
/// This is deliberately separate from [`CitationKey`] validation: imported,
/// explicit, and existing keys need not follow this naming convention.
pub fn generated_key(reference: &Reference) -> Result<CitationKey> {
    let author = reference.authors.first().ok_or_else(|| {
        anyhow::anyhow!(
            "cannot generate a citation key without an author\n\
             hint: provide an author or specify a citation key with `--key`"
        )
    })?;
    let year = reference.year.ok_or_else(|| {
        anyhow::anyhow!(
            "cannot generate a citation key without a publication year\n\
             hint: provide `--year` or specify a citation key with `--key`"
        )
    })?;

    let author_component: String = author
        .family
        .split_whitespace()
        .map(normalize_name_part)
        .collect();
    if author_component.is_empty() {
        bail!("cannot generate a citation key from the first author's family name\nhint: specify a citation key with `--key`");
    }

    let words: Vec<&str> = reference
        .title
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    let capitals: Vec<&str> = words
        .iter()
        .copied()
        .filter(|word| {
            word.chars()
                .find(|c| c.is_alphabetic())
                .is_some_and(char::is_uppercase)
        })
        .collect();
    let selected = if capitals.len() >= 2 {
        &capitals
    } else {
        &words
    };
    let title_component: String = selected
        .iter()
        .take(2)
        .map(|word| normalize_title_word(word))
        .collect();
    if title_component.is_empty() {
        bail!("cannot generate a citation key because the title contains no usable words\nhint: specify a citation key with `--key`");
    }

    CitationKey::new(format!("{author_component}{year}{title_component}"))
}

fn normalize_name_part(value: &str) -> String {
    let ascii = transliterate_latin(value);
    let filtered: String = ascii.chars().filter(char::is_ascii_alphanumeric).collect();
    let uniformly_cased = filtered
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .all(char::is_uppercase)
        || filtered
            .chars()
            .filter(|c| c.is_ascii_alphabetic())
            .all(char::is_lowercase);
    capitalize_ascii(if uniformly_cased {
        filtered.to_ascii_lowercase()
    } else {
        filtered
    })
}

fn normalize_title_word(value: &str) -> String {
    let filtered: String = transliterate_latin(value)
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect();
    capitalize_ascii(filtered)
}

fn capitalize_ascii(mut value: String) -> String {
    if let Some(first) = value.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    value
}

// Deterministic Latin transliteration for the common diacritics encountered in
// bibliographic names and titles. Characters without a useful ASCII equivalent
// are discarded by component normalization.
fn transliterate_latin(value: &str) -> String {
    value.chars().fold(String::new(), |mut output, c| {
        let replacement = match c {
            'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' | 'Ā' | 'Ă' | 'Ą' => "A",
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => "a",
            'Ç' | 'Ć' | 'Ĉ' | 'Ċ' | 'Č' => "C",
            'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => "c",
            'Ď' | 'Đ' => "D",
            'ď' | 'đ' => "d",
            'È' | 'É' | 'Ê' | 'Ë' | 'Ē' | 'Ĕ' | 'Ė' | 'Ę' | 'Ě' => "E",
            'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => "e",
            'Ì' | 'Í' | 'Î' | 'Ï' | 'Ĩ' | 'Ī' | 'Ĭ' | 'Į' => "I",
            'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' => "i",
            'Ñ' | 'Ń' | 'Ņ' | 'Ň' => "N",
            'ñ' | 'ń' | 'ņ' | 'ň' => "n",
            'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' | 'Ø' | 'Ō' | 'Ŏ' | 'Ő' => "O",
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => "o",
            'Ř' | 'Ŕ' | 'Ŗ' => "R",
            'ř' | 'ŕ' | 'ŗ' => "r",
            'Ś' | 'Ŝ' | 'Ş' | 'Š' => "S",
            'ś' | 'ŝ' | 'ş' | 'š' => "s",
            'Ť' | 'Ţ' => "T",
            'ť' | 'ţ' => "t",
            'Ù' | 'Ú' | 'Û' | 'Ü' | 'Ũ' | 'Ū' | 'Ŭ' | 'Ů' | 'Ű' | 'Ų' => "U",
            'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => "u",
            'Ý' | 'Ÿ' => "Y",
            'ý' | 'ÿ' => "y",
            'Ź' | 'Ż' | 'Ž' => "Z",
            'ź' | 'ż' | 'ž' => "z",
            'Æ' => "AE",
            'æ' => "ae",
            'Œ' => "OE",
            'œ' => "oe",
            'ß' => "ss",
            _ => "",
        };
        if c.is_ascii() {
            output.push(c);
        } else {
            output.push_str(replacement);
        }
        output
    })
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
        let reference = Reference {
            entry_type: ReferenceType::Article,
            title: "Relevance Feedback in Information Retrieval".into(),
            authors: vec![Person {
                given: "Joseph".into(),
                family: "Rocchio".into(),
            }],
            year: Some(1971),
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
        assert_eq!(
            generated_key(&reference).unwrap().as_str(),
            "Rocchio1971RelevanceFeedback"
        );
    }
    #[test]
    fn unknown_type() {
        assert!("journall".parse::<ReferenceType>().is_err());
    }
}
