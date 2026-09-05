use super::csl::CslItem;
use crate::model::Reference;
use std::{error::Error, fmt, process::Command, str::FromStr, time::Duration};

pub const CSL_JSON: &str = "application/vnd.citationstyles.csl+json";

/// A validated, canonical DOI. DOI equality is case-insensitive, so storage is lowercase.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Doi(String);

impl Doi {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Doi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for Doi {
    type Err = LookupError;
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let original = input.trim();
        let mut value = original;
        if value
            .get(..4)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("doi:"))
        {
            value = value[4..].trim_start();
        } else {
            for prefix in [
                "https://doi.org/",
                "http://doi.org/",
                "https://dx.doi.org/",
                "http://dx.doi.org/",
            ] {
                if value
                    .get(..prefix.len())
                    .is_some_and(|candidate| candidate.eq_ignore_ascii_case(prefix))
                {
                    value = &value[prefix.len()..];
                    break;
                }
            }
        }
        value = value.trim();
        let valid = value.split_once('/').is_some_and(|(prefix, suffix)| {
            prefix.strip_prefix("10.").is_some_and(|registrant| {
                !registrant.is_empty() && registrant.bytes().all(|b| b.is_ascii_digit())
            }) && !suffix.is_empty()
                && !suffix.chars().any(char::is_whitespace)
        });
        if !valid {
            return Err(LookupError::InvalidDoi(original.to_owned()));
        }
        Ok(Self(value.to_lowercase()))
    }
}

#[derive(Debug)]
pub enum LookupError {
    InvalidDoi(String),
    NotFound(Doi),
    RateLimited,
    ServiceFailure(u16),
    Retrieval { doi: Doi, source: anyhow::Error },
    InvalidMetadata { doi: Doi, source: anyhow::Error },
    IncompleteMetadata { doi: Doi, detail: String },
}

impl fmt::Display for LookupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDoi(v) => write!(f, "invalid DOI: {v}"),
            Self::NotFound(d) => write!(f, "DOI not found: {d}"),
            Self::RateLimited => {
                f.write_str("DOI metadata service temporarily rate-limited the request")
            }
            Self::ServiceFailure(s) => {
                write!(f, "DOI metadata service failed temporarily (HTTP {s})")
            }
            Self::Retrieval { doi, .. } => write!(f, "failed to retrieve metadata for DOI {doi}"),
            Self::InvalidMetadata { doi, .. } => {
                write!(f, "DOI resolver returned invalid metadata for {doi}")
            }
            Self::IncompleteMetadata { doi, detail } => {
                write!(f, "metadata for DOI {doi} is incomplete: {detail}")
            }
        }
    }
}
impl Error for LookupError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Retrieval { source, .. } | Self::InvalidMetadata { source, .. } => {
                Some(source.as_ref())
            }
            _ => None,
        }
    }
}

/// DOI content-negotiation client. `curl` supplies portable TLS and redirect handling
/// without making the otherwise synchronous CLI depend on an async runtime.
pub struct DoiMetadataClient {
    resolver: String,
    timeout: Duration,
}

impl DoiMetadataClient {
    pub fn new() -> Result<Self, LookupError> {
        Self::with_resolver("https://doi.org/", Duration::from_secs(15))
    }
    pub fn with_resolver(resolver: &str, timeout: Duration) -> Result<Self, LookupError> {
        if !(resolver.starts_with("https://") || resolver.starts_with("http://")) {
            return Err(LookupError::InvalidMetadata {
                doi: Doi("10.0/resolver".into()),
                source: anyhow::anyhow!("resolver must be an HTTP(S) URL"),
            });
        }
        Ok(Self {
            resolver: format!("{}/", resolver.trim_end_matches('/')),
            timeout,
        })
    }

    pub fn lookup(&self, doi: &Doi) -> Result<Reference, LookupError> {
        let url = format!("{}{}", self.resolver, encode_path(doi.as_str()));
        let output = Command::new("curl")
            .args([
                "--silent",
                "--show-error",
                "--location",
                "--max-redirs",
                "10",
                "--max-time",
            ])
            .arg(self.timeout.as_secs_f64().to_string())
            .args([
                "--header",
                &format!("Accept: {CSL_JSON}"),
                "--user-agent",
                concat!("ref/", env!("CARGO_PKG_VERSION")),
                "--write-out",
                "\n%{http_code}\n%{content_type}",
                "--url",
                &url,
            ])
            .output()
            .map_err(|e| LookupError::Retrieval {
                doi: doi.clone(),
                source: e.into(),
            })?;
        if !output.status.success() {
            return Err(LookupError::Retrieval {
                doi: doi.clone(),
                source: anyhow::anyhow!(
                    "curl failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            });
        }
        let text = String::from_utf8(output.stdout).map_err(|e| LookupError::InvalidMetadata {
            doi: doi.clone(),
            source: e.into(),
        })?;
        let (body_and_status, content_type) = text
            .rsplit_once('\n')
            .ok_or_else(|| invalid(doi, "missing response metadata"))?;
        let (body, status) = body_and_status
            .rsplit_once('\n')
            .ok_or_else(|| invalid(doi, "missing HTTP status"))?;
        let status: u16 = status.parse().map_err(|e| LookupError::InvalidMetadata {
            doi: doi.clone(),
            source: anyhow::Error::new(e),
        })?;
        match status {
            404 => return Err(LookupError::NotFound(doi.clone())),
            429 => return Err(LookupError::RateLimited),
            500..=599 => return Err(LookupError::ServiceFailure(status)),
            200..=299 => {}
            _ => {
                return Err(LookupError::Retrieval {
                    doi: doi.clone(),
                    source: anyhow::anyhow!("resolver returned HTTP {status}"),
                })
            }
        }
        if !(content_type.starts_with("application/json") || content_type.starts_with(CSL_JSON)) {
            return Err(invalid(
                doi,
                &format!("unexpected content type `{content_type}`"),
            ));
        }
        let item: CslItem =
            serde_yaml::from_str(body).map_err(|e| LookupError::InvalidMetadata {
                doi: doi.clone(),
                source: e.into(),
            })?;
        item.into_reference(doi)
            .map_err(|detail| LookupError::IncompleteMetadata {
                doi: doi.clone(),
                detail,
            })
    }
}

fn invalid(doi: &Doi, detail: &str) -> LookupError {
    LookupError::InvalidMetadata {
        doi: doi.clone(),
        source: anyhow::anyhow!(detail.to_owned()),
    }
}
fn encode_path(value: &str) -> String {
    value
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'~' | b'/') {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };
    const COMPLETE: &str = r#"{"type":"article-journal","title":"Example","author":[{"given":"Jane","family":"Doe"}],"DOI":"10.1234/example","issued":{"date-parts":[[2024]]}}"#;
    fn server(
        status: &str,
        content_type: &str,
        body: &str,
        delay: Duration,
    ) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (status, content_type, body) =
            (status.to_owned(), content_type.to_owned(), body.to_owned());
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut bytes = [0; 4096];
            let size = stream.read(&mut bytes).unwrap();
            let request = String::from_utf8_lossy(&bytes[..size]).into_owned();
            thread::sleep(delay);
            let _ = write!(stream,"HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
            request
        });
        (format!("http://{address}/"), handle)
    }
    #[test]
    fn normalizes_common_doi_forms() {
        for v in [
            "10.1234/example",
            "doi:10.1234/example",
            "DOI: 10.1234/EXAMPLE",
            "https://doi.org/10.1234/example",
            "http://doi.org/10.1234/example",
            "https://dx.doi.org/10.1234/example",
            "  10.1234/example  ",
        ] {
            assert_eq!(v.parse::<Doi>().unwrap().as_str(), "10.1234/example");
        }
    }
    #[test]
    fn rejects_invalid_dois() {
        for v in ["foo", "10", "10.", "10.1234", "https://example.com/foo"] {
            assert!(v.parse::<Doi>().is_err());
        }
    }
    #[test]
    fn sends_header_and_parses_success() {
        let (url, h) = server("200 OK", CSL_JSON, COMPLETE, Duration::ZERO);
        let r = DoiMetadataClient::with_resolver(&url, Duration::from_secs(1))
            .unwrap()
            .lookup(&"10.1234/example".parse().unwrap())
            .unwrap();
        assert_eq!(r.title, "Example");
        assert!(h
            .join()
            .unwrap()
            .to_ascii_lowercase()
            .contains(&format!("accept: {CSL_JSON}").to_ascii_lowercase()));
    }
    #[test]
    fn categorizes_failures() {
        for (s, c, b, e) in [
            ("404 Not Found", "text/plain", "", "not found"),
            ("429 Too Many Requests", "text/plain", "", "rate-limited"),
            (
                "500 Internal Server Error",
                "text/plain",
                "",
                "failed temporarily",
            ),
            ("200 OK", CSL_JSON, "{bad", "invalid metadata"),
            ("200 OK", "text/html", "<html/>", "invalid metadata"),
            (
                "200 OK",
                CSL_JSON,
                r#"{"title":"Incomplete"}"#,
                "incomplete",
            ),
        ] {
            let (url, h) = server(s, c, b, Duration::ZERO);
            let err = DoiMetadataClient::with_resolver(&url, Duration::from_secs(1))
                .unwrap()
                .lookup(&"10.1234/example".parse().unwrap())
                .unwrap_err();
            assert!(err.to_string().contains(e), "{err}");
            h.join().unwrap();
        }
    }
    #[test]
    fn times_out() {
        let (url, h) = server("200 OK", CSL_JSON, COMPLETE, Duration::from_millis(100));
        let e = DoiMetadataClient::with_resolver(&url, Duration::from_millis(10))
            .unwrap()
            .lookup(&"10.1234/example".parse().unwrap())
            .unwrap_err();
        assert!(matches!(e, LookupError::Retrieval { .. }));
        h.join().unwrap();
    }
    #[test]
    fn follows_redirects() {
        let (dest, dh) = server("200 OK", CSL_JSON, COMPLETE, Duration::ZERO);
        let location = format!("{dest}10.1234/example");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let source = format!("http://{}/", listener.local_addr().unwrap());
        let rh = thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut b = [0; 1024];
            let _ = s.read(&mut b);
            write!(
                s,
                "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\n\r\n"
            )
            .unwrap();
        });
        assert_eq!(
            DoiMetadataClient::with_resolver(&source, Duration::from_secs(1))
                .unwrap()
                .lookup(&"10.1234/example".parse().unwrap())
                .unwrap()
                .title,
            "Example"
        );
        rh.join().unwrap();
        dh.join().unwrap();
    }
}
