use crate::repository::StoredReference;

pub fn biblatex(refs: &[StoredReference]) -> String {
    let mut refs = refs.to_vec();
    refs.sort_by(|a, b| a.key.cmp(&b.key));
    refs.iter().map(entry).collect::<Vec<_>>().join("\n")
}

fn entry(stored: &StoredReference) -> String {
    let r = &stored.metadata;
    let mut fields = vec![
        (
            "author",
            r.authors
                .iter()
                .map(|a| format!("{}, {}", escape(&a.family), escape(&a.given)))
                .collect::<Vec<_>>()
                .join(" and "),
        ),
        ("title", escape(&r.title)),
    ];
    if let Some(v) = &r.container_title {
        fields.push((
            if matches!(r.entry_type, crate::model::ReferenceType::Article) {
                "journaltitle"
            } else {
                "booktitle"
            },
            escape(v),
        ));
    }
    if let Some(v) = r.year {
        fields.push(("year", v.to_string()));
    }
    for (name, value) in [
        ("publisher", r.publisher.as_ref()),
        ("volume", r.volume.as_ref()),
        ("number", r.issue.as_ref()),
        ("doi", r.doi.as_ref()),
        ("url", r.url.as_ref()),
    ] {
        if let Some(v) = value {
            fields.push((name, escape(v)));
        }
    }
    if let Some(v) = &r.pages {
        fields.push(("pages", normalize_pages(v)));
    }
    let width = fields.iter().map(|(n, _)| n.len()).max().unwrap_or(0);
    let body = fields
        .into_iter()
        .map(|(n, v)| format!("  {n:width$} = {{{v}}},", width = width))
        .collect::<Vec<_>>()
        .join("\n");
    format!("@{}{{{},\n{}\n}}\n", r.entry_type, stored.key, body)
}

fn escape(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\textbackslash{}"),
            '&' | '%' | '$' | '#' | '_' | '{' | '}' => {
                out.push('\\');
                out.push(c)
            }
            '~' => out.push_str("\\textasciitilde{}"),
            '^' => out.push_str("\\textasciicircum{}"),
            _ => out.push(c),
        }
    }
    out
}
fn normalize_pages(s: &str) -> String {
    let p: Vec<_> = s.split('-').collect();
    if p.len() == 2
        && p.iter()
            .all(|x| !x.is_empty() && x.chars().all(|c| c.is_ascii_digit()))
    {
        format!("{}--{}", p[0], p[1])
    } else {
        escape(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::{CitationKey, Person, Reference, ReferenceType},
        repository::StoredReference,
    };
    use std::path::PathBuf;
    fn item(key: &str) -> StoredReference {
        StoredReference {
            key: CitationKey::new(key).unwrap(),
            path: PathBuf::new(),
            has_pdf: false,
            added_at: None,
            metadata: Reference {
                entry_type: ReferenceType::Article,
                title: "A & B".into(),
                authors: vec![Person {
                    given: "Jane".into(),
                    family: "Smith".into(),
                }],
                year: Some(2024),
                container_title: Some("Journal".into()),
                publisher: None,
                volume: None,
                issue: None,
                pages: Some("12-19".into()),
                doi: Some("10.1/x".into()),
                url: None,
                tags: vec![],
                notes: None,
            },
        }
    }
    // Scenario: export is escaped and sorted.
    // Requirements: REQ-050, REQ-051
    #[test]
    fn export_is_escaped_and_sorted() {
        let out = biblatex(&[item("z"), item("a")]);
        assert!(out.starts_with("@article{a,"));
        assert!(out.contains("A \\& B"));
        assert!(out.contains("12--19"));
        assert!(out.contains("Smith, Jane"));
    }
}
