//! Shared, bounded URL validation and reachability checks.
use anyhow::{bail, Result};
use std::{process::Command, time::Duration};
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UrlStatus {
    Reachable { final_url: String, redirected: bool },
    ClientError(u16),
    ServerError(u16),
    Timeout,
    ConnectionFailure(String),
}
impl UrlStatus {
    pub fn is_reachable(&self) -> bool {
        matches!(self, Self::Reachable { .. })
    }
}
pub trait UrlChecker {
    fn check(&self, url: &str) -> Result<UrlStatus>;
}
#[derive(Clone, Debug)]
pub struct HttpUrlChecker {
    timeout: Duration,
}
impl Default for HttpUrlChecker {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(10),
        }
    }
}
impl HttpUrlChecker {
    pub fn new(timeout: Duration) -> Self {
        Self { timeout }
    }
    fn request(&self, value: &str, head: bool) -> UrlStatus {
        let marker = "__REF_URL__";
        let timeout = self.timeout.as_secs_f64().to_string();
        let write_out = format!("{marker}%{{http_code}} %{{url_effective}}");
        let mut command = Command::new("curl");
        command.args([
            "--silent",
            "--show-error",
            "--location",
            "--max-redirs",
            "5",
            "--max-time",
            &timeout,
            "--output",
            "/dev/null",
            "--write-out",
            &write_out,
        ]);
        if head {
            command.arg("--head");
        } else {
            command.args(["--range", "0-0"]);
        }
        let output = match command.arg(value).output() {
            Ok(v) => v,
            Err(e) => return UrlStatus::ConnectionFailure(format!("cannot run curl: {e}")),
        };
        if !output.status.success() {
            return if output.status.code() == Some(28) {
                UrlStatus::Timeout
            } else {
                UrlStatus::ConnectionFailure(
                    String::from_utf8_lossy(&output.stderr).trim().to_owned(),
                )
            };
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let result = stdout
            .rsplit_once(marker)
            .map(|(_, v)| v)
            .unwrap_or_default();
        let (code, final_url) = result.trim().split_once(' ').unwrap_or(("0", value));
        let code = code.parse::<u16>().unwrap_or(0);
        match code {
            200..=299 => UrlStatus::Reachable {
                final_url: final_url.into(),
                redirected: final_url != value,
            },
            400..=499 => UrlStatus::ClientError(code),
            500..=599 => UrlStatus::ServerError(code),
            _ => UrlStatus::ConnectionFailure(format!("unexpected HTTP status {code}")),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedUrl {
    host: String,
}
impl ParsedUrl {
    pub fn host(&self) -> &str {
        &self.host
    }
}
pub fn validate_url(value: &str) -> Result<ParsedUrl> {
    let rest = value
        .strip_prefix("http://")
        .or_else(|| value.strip_prefix("https://"))
        .ok_or_else(|| {
            anyhow::anyhow!("invalid URL `{value}`: expected an absolute HTTP or HTTPS URL")
        })?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let hp = authority.rsplit('@').next().unwrap_or_default();
    let host = hp
        .trim_start_matches('[')
        .split([']', ':'])
        .next()
        .unwrap_or_default();
    if host.is_empty()
        || host.chars().any(char::is_whitespace)
        || (!host.contains('.') && host != "localhost")
    {
        bail!("invalid URL `{value}`: invalid or missing host");
    }
    Ok(ParsedUrl {
        host: host.to_owned(),
    })
}
impl UrlChecker for HttpUrlChecker {
    fn check(&self, value: &str) -> Result<UrlStatus> {
        validate_url(value)?;
        let s = self.request(value, true);
        if matches!(s, UrlStatus::ClientError(403 | 405 | 501)) {
            Ok(self.request(value, false))
        } else {
            Ok(s)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };
    fn server(responses: Vec<&'static str>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            for response in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut b = [0; 1024];
                let _ = stream.read(&mut b);
                stream.write_all(response.as_bytes()).unwrap();
            }
        });
        format!("http://localhost:{}", addr.port())
    }
    // Scenario: only syntactically valid absolute HTTP(S) URLs are accepted. Requirement: REQ-114.
    #[test]
    fn syntax_validation() {
        assert!(validate_url("https://example.org/a").is_ok());
        for v in ["example.org", "file:///tmp/x", "https://"] {
            assert!(validate_url(v).is_err());
        }
    }
    // Scenario: a local 2xx response is reachable without public network access. Requirement: REQ-116.
    #[test]
    fn success() {
        let u = server(vec!["HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n"]);
        assert!(HttpUrlChecker::new(Duration::from_secs(1))
            .check(&u)
            .unwrap()
            .is_reachable());
    }
    // Scenario: redirects are followed and reported with their final URL. Requirement: REQ-116.
    #[test]
    fn redirect() {
        let base = server(vec![
            "HTTP/1.1 302 Found\r\nLocation: /final\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n",
        ]);
        assert!(matches!(
            HttpUrlChecker::new(Duration::from_secs(1))
                .check(&base)
                .unwrap(),
            UrlStatus::Reachable {
                redirected: true,
                ..
            }
        ));
    }
    // Scenario: client and server failures are classified rather than accepted. Requirement: REQ-116.
    #[test]
    fn broken() {
        let u = server(vec!["HTTP/1.1 500 Nope\r\nConnection: close\r\n\r\n"]);
        assert_eq!(
            HttpUrlChecker::new(Duration::from_secs(1))
                .check(&u)
                .unwrap(),
            UrlStatus::ServerError(500)
        );
    }
    // Scenario: a bounded local request reports a timeout. Requirement: REQ-116.
    #[test]
    fn timeout() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let u = format!("http://localhost:{}", listener.local_addr().unwrap().port());
        thread::spawn(move || {
            let (_stream, _) = listener.accept().unwrap();
            thread::sleep(Duration::from_secs(1));
        });
        assert_eq!(
            HttpUrlChecker::new(Duration::from_millis(50))
                .check(&u)
                .unwrap(),
            UrlStatus::Timeout
        );
    }
    // Scenario: rejected HEAD falls back to a minimal GET. Requirement: REQ-116.
    #[test]
    fn head_fallback() {
        let u = server(vec![
            "HTTP/1.1 405 Nope\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n",
        ]);
        assert!(HttpUrlChecker::new(Duration::from_secs(1))
            .check(&u)
            .unwrap()
            .is_reachable());
    }
}
