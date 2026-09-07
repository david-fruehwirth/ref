use super::doi::Doi;
use crate::model::{Person, Reference, ReferenceType};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct CslItem {
    #[serde(rename = "type")]
    item_type: Option<String>,
    title: Option<String>,
    author: Option<Vec<CslName>>,
    #[serde(rename = "container-title")]
    container_title: Option<String>,
    publisher: Option<String>,
    volume: Option<String>,
    issue: Option<String>,
    page: Option<String>,
    #[serde(rename = "DOI")]
    doi: Option<String>,
    url: Option<String>,
    issued: Option<CslDate>,
}

#[derive(Debug, Deserialize)]
struct CslName {
    given: Option<String>,
    family: Option<String>,
    literal: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CslDate {
    #[serde(rename = "date-parts")]
    date_parts: Option<Vec<Vec<u16>>>,
}

impl CslItem {
    pub fn into_reference(self, requested_doi: &Doi) -> Result<Reference, String> {
        if let Some(returned) = self.doi.as_deref() {
            let returned = returned
                .parse::<Doi>()
                .map_err(|_| "resolver returned an invalid DOI".to_owned())?;
            if &returned != requested_doi {
                return Err("resolver returned a different DOI".to_owned());
            }
        }
        let title = required_text(self.title, "title")?;
        let authors: Vec<Person> = self
            .author
            .unwrap_or_default()
            .into_iter()
            .filter_map(|name| {
                if let Some(literal) = nonempty(name.literal) {
                    return Some(Person {
                        given: String::new(),
                        family: literal,
                    });
                }
                nonempty(name.family).map(|family| Person {
                    given: nonempty(name.given).unwrap_or_default(),
                    family,
                })
            })
            .collect();
        if authors.is_empty() {
            return Err("missing required field: author".to_owned());
        }
        let year = self
            .issued
            .and_then(|date| date.date_parts)
            .and_then(|parts| parts.first().and_then(|part| part.first()).copied())
            .ok_or_else(|| "missing required field: publication year".to_owned())?;
        let reference = Reference {
            entry_type: map_type(self.item_type.as_deref()),
            title,
            authors,
            year: Some(year),
            container_title: nonempty(self.container_title),
            publisher: nonempty(self.publisher),
            volume: nonempty(self.volume),
            issue: nonempty(self.issue),
            pages: nonempty(self.page),
            doi: Some(requested_doi.to_string()),
            url: nonempty(self.url),
            tags: vec![],
            notes: None,
        };
        reference.validate().map_err(|error| error.to_string())?;
        Ok(reference)
    }
}

fn nonempty(value: Option<String>) -> Option<String> {
    value.and_then(|value| (!value.trim().is_empty()).then(|| value.trim().to_owned()))
}

fn required_text(value: Option<String>, name: &str) -> Result<String, String> {
    nonempty(value).ok_or_else(|| format!("missing required field: {name}"))
}

fn map_type(value: Option<&str>) -> ReferenceType {
    match value {
        Some("article-journal") => ReferenceType::Article,
        Some("paper-conference") => ReferenceType::Inproceedings,
        Some("chapter") => ReferenceType::Incollection,
        Some("book") => ReferenceType::Book,
        Some("report") => ReferenceType::Report,
        Some("thesis") => ReferenceType::Thesis,
        _ => ReferenceType::Misc,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn convert(json: &str) -> Result<Reference, String> {
        serde_yaml::from_str::<CslItem>(json)
            .unwrap()
            .into_reference(&"10.1234/example".parse().unwrap())
    }

    // Scenario: converts complete article and dates.
    // Requirement: REQ-057
    #[test]
    fn converts_complete_article_and_dates() {
        for date in ["[2024]", "[2024,5]", "[2024,5,10]"] {
            let json = format!(
                r#"{{"type":"article-journal","title":"Example Article","author":[{{"given":"Jane","family":"Doe"}},{{"literal":"World Health Organization"}}],"container-title":"Journal of Examples","volume":"12","issue":"3","page":"100-112","DOI":"10.1234/EXAMPLE","issued":{{"date-parts":[{date}]}}}}"#
            );
            let reference = convert(&json).unwrap();
            assert_eq!(reference.entry_type, ReferenceType::Article);
            assert_eq!(reference.title, "Example Article");
            assert_eq!(reference.authors.len(), 2);
            assert_eq!(reference.authors[1].family, "World Health Organization");
            assert_eq!(reference.year, Some(2024));
            assert_eq!(
                reference.container_title.as_deref(),
                Some("Journal of Examples")
            );
            assert_eq!(reference.volume.as_deref(), Some("12"));
            assert_eq!(reference.issue.as_deref(), Some("3"));
            assert_eq!(reference.pages.as_deref(), Some("100-112"));
            assert_eq!(reference.doi.as_deref(), Some("10.1234/example"));
        }
    }

    // Scenario: maps supported and unknown types.
    // Requirement: REQ-058
    #[test]
    fn maps_supported_and_unknown_types() {
        for (csl, expected) in [
            ("article-journal", ReferenceType::Article),
            ("paper-conference", ReferenceType::Inproceedings),
            ("chapter", ReferenceType::Incollection),
            ("book", ReferenceType::Book),
            ("report", ReferenceType::Report),
            ("thesis", ReferenceType::Thesis),
            ("dataset", ReferenceType::Misc),
        ] {
            let json = format!(
                r#"{{"type":"{csl}","title":"T","author":[{{"family":"Doe"}}],"issued":{{"date-parts":[[2024]]}}}}"#
            );
            assert_eq!(convert(&json).unwrap().entry_type, expected);
        }
    }

    // Scenario: rejects missing required fields and malformed dates.
    // Requirement: REQ-059
    #[test]
    fn rejects_missing_required_fields_and_malformed_dates() {
        for json in [
            r#"{"author":[{"family":"Doe"}],"issued":{"date-parts":[[2024]]}}"#,
            r#"{"title":"T","issued":{"date-parts":[[2024]]}}"#,
            r#"{"title":"T","author":[{"family":"Doe"}],"issued":{"date-parts":[]}}"#,
            r#"{"title":"T","author":[{"family":"Doe"}],"issued":{"date-parts":[[]]}}"#,
        ] {
            assert!(convert(json).is_err());
        }
    }
}
