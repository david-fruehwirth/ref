# Security Policy

## Supported Versions

Security fixes are provided for the latest released version of `ref`.

## Reporting a Vulnerability

Please report suspected security vulnerabilities privately by using GitHub Security Advisories:

https://github.com/david-fruehwirth/ref/security/advisories/new

Please include:

- the affected `ref` version or commit;
- your operating system;
- steps to reproduce;
- the expected and actual impact;
- any relevant sample files, if safe to share.

Do not include sensitive bibliography data, unpublished research, private PDFs, or credentials in reports unless necessary.

## Scope

Examples of security-sensitive issues include:

- path traversal or writes outside the intended `.ref` repository;
- unsafe deletion or overwrite of user files;
- command execution through editor/viewer invocation;
- malicious BibTeX, YAML, DOI, URL, or PDF metadata causing unintended behavior;
- network metadata lookup exposing more information than expected;
- denial-of-service inputs that cause excessive resource use.

Normal malformed input, validation errors, or crashes without a security impact can be reported as regular bugs.

## Disclosure

Please allow a reasonable amount of time for investigation and a fix before public disclosure.
