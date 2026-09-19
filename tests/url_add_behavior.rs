mod support;
use predicates::prelude::*;
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    thread,
};
use support::command;
fn server(response: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut b = [0; 1024];
        let _ = stream.read(&mut b);
        stream.write_all(response.as_bytes()).unwrap();
    });
    format!("http://localhost:{}", addr.port())
}
// Scenario: URL add creates an online authorless scaffold with automatic access date and honors explicit metadata. Requirements: REQ-114, REQ-115.
#[test]
fn url_add_creates_metadata_and_overrides_defaults() {
    let t = tempfile::tempdir().unwrap();
    command(t.path()).arg("init").assert().success();
    let url = server("HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n");
    command(t.path())
        .args([
            "add",
            "--url",
            &url,
            "--title",
            "Explicit title",
            "--year",
            "2026",
        ])
        .assert()
        .success();
    let refs = t.path().join(".ref/refs");
    let path = fs::read_dir(refs)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path()
        .join("ref.yaml");
    let yaml = fs::read_to_string(path).unwrap();
    assert!(
        yaml.contains("type: online")
            && yaml.contains("title: Explicit title")
            && yaml.contains("authors: []")
            && yaml.contains("year: 2026")
            && yaml.contains("urldate: ")
    );
}
// Scenario: unreachable URL creation fails before any reference directory is created. Requirement: REQ-114.
#[test]
fn unreachable_url_is_atomic() {
    let t = tempfile::tempdir().unwrap();
    command(t.path()).arg("init").assert().success();
    let url = server("HTTP/1.1 500 Broken\r\nConnection: close\r\n\r\n");
    command(t.path())
        .args(["add", "--url", &url])
        .assert()
        .failure();
    assert_eq!(fs::read_dir(t.path().join(".ref/refs")).unwrap().count(), 0);
}
// Scenario: doctor can skip all stored-URL network traffic while retaining local checks. Requirement: REQ-117.
#[test]
fn doctor_no_network_skips_url_checks() {
    let t = tempfile::tempdir().unwrap();
    command(t.path()).arg("init").assert().success();
    fs::create_dir(t.path().join(".ref/refs/offline")).unwrap();
    fs::write(
        t.path().join(".ref/refs/offline/ref.yaml"),
        "type: online\ntitle: Offline\nauthors: []\nurl: https://does-not-resolve.invalid/x\n",
    )
    .unwrap();
    command(t.path())
        .args(["doctor", "--no-network"])
        .assert()
        .success()
        .stdout(predicates::str::contains("connection/DNS/TLS").not());
}
