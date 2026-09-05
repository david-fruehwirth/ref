# AGENTS.md

This file is the repository-level engineering constitution for coding agents. It
describes how to reason about `ref`; the [README](README.md) describes how users
operate it. Read this file before changing code or persistent data formats.

## Project Overview

`ref` is a small, CLI-native reference manager written in Rust for researchers
whose scientific-writing projects—especially LaTeX theses and papers—live in
Git. It deliberately avoids the breadth and hidden state of general-purpose
managers such as Zotero, JabRef, or Papis.

The primary abstraction is a project-local `.ref` directory containing readable
YAML metadata and, optionally, one PDF per reference. **The filesystem is the
database.** The executable is a safe, convenient interface over files that
remain useful if `ref` disappears.

A typical workflow is:

```bash
ref init
ref add paper.pdf
ref search rocchio
ref open rocchio1971
ref export > references.bib
ref doctor
```

Migration from an existing project begins with `ref import existing.bib`.

> **Core principle:** `.ref` is the database. YAML files are authoritative.
> Everything else is an interface, derived representation, or disposable output.

> **Data ownership principle:** the user's bibliography must never become trapped
> inside an opaque database or application-specific service.

> **Safety principle:** when ambiguity could modify or delete the wrong reference,
> require explicit identity rather than guessing.

## Product Philosophy

### Human-readable, local-first state

All important persistent state MUST be understandable without running `ref`.
Users can inspect `.ref/refs/<key>/ref.yaml` in any editor. Normal operation
requires no server, account, service, or network connection; a repository belongs
to its writing project.

Manual modification of `.ref` is supported behavior, not tampering. Editing
`.ref/refs/rocchio1971/ref.yaml` or copying a valid `.ref/refs/example2024/`
directory into place must be observed on the next command. `doctor` is the safety
net for errors introduced through such edits.

### Version-control-friendly conventions

A one-reference metadata edit should normally change one small YAML file and
produce a meaningful Git diff. Generated output SHOULD be deterministic. Prefer
the fixed layout `refs/<key>/ref.yaml` and `refs/<key>/paper.pdf` to configurable
storage paths. Repository-wide configuration is intentionally sparse.

### Composable CLI, small scope

`ref export > references.bib`, `ref search attention`, and `ref doctor --strict`
are first-class workflows. stdout, stderr, and exit statuses MUST remain useful
to shell scripts and CI. Prefer short, explicit commands to a large interactive
interface. Do not grow `ref` into a general academic knowledge-management system.

### Prefer boring technology

Ordinary Rust structs, enums, modules, filesystem operations, and deterministic
transformations are usually enough. Do not add infrastructure because it is
fashionable. Optimize only when measurements demonstrate a user-visible problem.

## The Git Analogy

`ref` borrows its mental model from Git more than from desktop reference managers.
A Git project has visible files and a project-local `.git`; a `ref` project has
ordinary writing files and a project-local `.ref`. Commands discover the nearest
repository by walking parent directories, so they work anywhere below the project
root. Human-readable citation keys are stable identities used by explicit commands:

```bash
ref show rocchio1971
ref open rocchio1971
ref rename rocchio1971 rocchio1971feedback
```

Like Git, `ref` favors visible deterministic state, explicit operations, CLI
composition, and meaningful failures. The analogy is a UX influence, not an
implementation requirement: `ref` has no object database, hash identity, staging
area, or branch model, and does not require or use Git internally.

## Goals

- Provide a pleasant CLI for project-local reference management.
- Integrate naturally with Git-managed scientific-writing projects.
- Keep repository metadata readable, portable, and owned by the user.
- Manage one optional local PDF per reference and open it in the native viewer.
- Search efficiently by citation key, title, author, year, and tags.
- Generate deterministic BibLaTeX while preserving citation keys.
- Import existing BibTeX/BibLaTeX libraries without PDFs.
- Validate repositories with `doctor` and support scripts and CI.
- Remain small enough to understand, test, and maintain.

The scale target is a thesis-sized library: roughly 10–2,000 references. Scanning
directories and parsing YAML in memory is appropriate; do not optimize for millions
of records prematurely.

## Non-Goals and What `ref` Is Not

`ref` is not an attempt to replace every Zotero feature. Zotero optimizes for a
rich managed application; `ref` optimizes for transparent files, terminal use,
Git, and data ownership.

It is not currently a GUI, PDF reader, annotation manager, cloud-sync platform,
multi-user database, note-taking system, knowledge graph, recommendation engine,
browser extension, web service, or collaborative bibliography server.

Unless adopted through an intentional future design, it also excludes SQLite,
PostgreSQL, authoritative binary indexes, background daemons, accounts, mandatory
remote APIs, full-text PDF indexing, OCR, annotation synchronization, automatic
LaTeX citation rewriting, automatic metadata merging, and silent citation-key
renaming. Changing a non-goal requires an explicit architectural decision, not
incidental implementation convenience. When choosing between another subsystem
and preserving simplicity, prefer simplicity unless the capability clearly
strengthens the core workflow.

## Repository Model

The canonical layout is:

```text
project/
├── .git/
├── .ref/
│   ├── config.yaml
│   └── refs/
│       ├── rocchio1971/
│       │   ├── ref.yaml
│       │   └── paper.pdf
│       └── lops2011/
│           └── ref.yaml
├── thesis.tex
└── references.bib
```

- `.ref/config.yaml` contains the repository format version and only genuinely
  repository-wide configuration. It is not a preferences dumping ground.
- `.ref/refs/` contains one directory per reference.
- The directory name is the citation key and canonical identity.
- `ref.yaml` is authoritative bibliographic and local metadata.
- `paper.pdf` is the optional canonical attachment. Exactly one PDF is currently
  supported per reference.

### The filesystem is part of the public interface

Users are expected to inspect, edit, copy, diff, and version `.ref` directly.
Therefore the layout and YAML schema are public contracts rather than private
implementation details. Keep filenames predictable, metadata readable, and
migration churn low. Treat schema changes with the same care as public API changes.

### One source of truth; no authoritative index

Avoid storing the same authoritative fact twice. A reference at
`.ref/refs/rocchio1971/` has identity `rocchio1971`; `ref.yaml` deliberately has no
independent `key:` field. Two copies of identity could disagree. Exported BibLaTeX
is not metadata, and a future cache may not become metadata.

`ref` deliberately has no required `index.yaml` or database index. At the intended
scale, scanning and parsing YAML is cheap. Avoiding an index removes synchronization
failures and makes manual edits and copies immediately effective. A cache MAY be
added only as a disposable, reconstructable optimization after profiling proves a
need. It MUST never become a source of truth.

### Authoritative versus derived state

Authoritative state is `.ref/config.yaml`, each
`.ref/refs/<key>/ref.yaml`, and optional reference attachments. Derived state
includes exported `references.bib`, terminal tables, search results, and any future
cache or index. Derived state MUST be reproducible from authoritative state.

### Repository compatibility and versioning

Repository compatibility is more important than internal Rust API compatibility.
Existing `.ref` repositories should remain readable whenever reasonably possible.
The `version: 1` value in `.ref/config.yaml` is the repository format version, not
the application version. Do not increment it for ordinary CLI changes. If a
schema-breaking storage change is unavoidable, increment the version, provide an
explicit migration, never silently reinterpret old metadata, and test migration.

## Core Invariants and Data Safety

Citation keys are repository identity and MUST remain stable because LaTeX files
may contain `\cite{rocchio1971}`. Import preserves source keys; rename is explicit;
collisions are not resolved by inventing imported keys. Mutating commands require
an exact, validated key.

Preserve data before convenience. Bibliographies can embody years of work; a less
convenient explicit command is preferable to a guess that corrupts state.

1. Never silently overwrite an existing reference.
2. Never silently rename an imported citation key.
3. Never delete a source PDF when `add` copies it.
4. Never delete or modify a source `.bib` during import.
5. Make mutation atomic per reference where practical.
6. Failed creation must not leave an apparently valid partial reference.
7. Existing metadata must survive failed writes.
8. Destructive operations require exact identity and appropriate confirmation.
9. Do not weaken domain validation merely to make an import succeed.
10. Do not introduce hidden authoritative state.

Citation keys MUST not allow path traversal. Convert untrusted identity through
`CitationKey` before constructing paths; values such as `..`, `foo/bar`, and
`foo\bar` must not escape `.ref/refs/`. Treat paths as hostile input at boundaries
and use `Path`/`PathBuf`, not hard-coded separators.

## Metadata Model

YAML is the persistence representation; typed domain structs are the application
representation; BibLaTeX is an export representation. Do not conflate these or
model the application as an arbitrary map of BibTeX fields. Current YAML resembles:

```yaml
type: article
title: Example Paper
authors:
  - given: Jane
    family: Smith
year: 2024
container_title: Journal of Examples
volume: "12"
issue: "3"
pages: 10-20
doi: 10.1234/example
url: https://example.org
tags:
  - recommender-systems
notes: |
  Relevant to the thesis methodology.
```

`model.rs` defines `CitationKey`, `ReferenceType`, `Reference`, and structured
`Person { given, family }` values. Author order is meaningful and MUST be preserved.
CLI syntax such as repeatable `--author "Jane, Smith"` is only an input format;
the exporter owns BibLaTeX author formatting. Domain values should retain meaning
independently of any export format.

## CLI Design Philosophy

- Prefer short verbs such as `add`, `open`, `show`, and `rm` over deep command trees.
- Search MAY use substring matching; mutation MUST use an exact citation key.
- Arguments and prompts compose. Supplied metadata is retained, and interactive
  `add` prompts only for unresolved inputs. A fully specified invocation should not
  prompt unnecessarily.
- Interactive `add` requests a publication year when absent, while allowing the
  user to leave an unknown year blank. Interactive, non-interactive, PDF, and
  `--no-pdf` paths converge on one validation/key-resolution/persistence flow.
- Normal results go to stdout; warnings and errors go to stderr. Failures MUST have
  non-zero status. Batch commands communicate partial failure.
- `open` delegates to the native PDF viewer. `edit` delegates to `$VISUAL`, then
  `$EDITOR`. Do not embed a large interactive UI without a product decision.

The intended add flow is:

```text
CLI-provided metadata
        ↓
resolve missing metadata interactively
        ↓
validate domain model
        ↓
resolve or generate citation key
        ↓
persist atomically
```

Current repeatable author syntax is `--author "Given Names, Family Name"`:

```bash
ref add paper.pdf \
  --title "Example Paper" \
  --author "Jane, Smith" \
  --author "John, Doe" \
  --year 2024
```

## Current Command Surface

The implemented shape is:

```text
ref init
ref add <PDF> [--key --title --author --year --type ...]
ref add --no-pdf [metadata flags]
ref list [--sort key|year|author|title]
ref search <QUERY> [--key|--author|--title|--year|--tag]
ref show <KEY>
ref open <KEY>
ref edit <KEY>
ref rename <OLD> <NEW>
ref remove <KEY> [--yes]      # `rm` alias
ref import <FILE>
ref export [biblatex] [--output <FILE>]
ref doctor [--strict]
ref --help
ref --version
```

This is orientation, not a substitute for `ref COMMAND --help`. Non-interactive
`add` requires `--title`; a key may be supplied or generated. Search is
case-insensitive and substring-based across key, authors, title, tags, and year,
with one optional field restriction. Results have deterministic relevance/key
ordering. Keep linear search unless evidence demands more; do not add embeddings,
vector databases, fuzzy-ranking frameworks, or a search service casually.

## Architecture

The crate is intentionally flat and its real module boundaries are:

```text
main.rs (Clap CLI and command/application flow)
    ↓
model.rs (domain values and validation)
    ↓
repository.rs (discovery and filesystem persistence)

import.rs: BibTeX/BibLaTeX text → domain values → Repository::add
export.rs: stored domain values → BibLaTeX text
launch.rs: injected viewer/editor/environment boundaries
```

- **CLI/application (`main.rs`)** parses arguments, resolves prompts, formats
  output, and maps results to statuses. Bibliography conversion does not belong
  here.
- **Domain (`model.rs`)** owns references, structured people, types, keys, key
  generation, and validation. It MUST NOT depend on terminal behavior.
- **Repository (`repository.rs`)** owns discovery, paths, structure validation,
  loading, atomic creation, rename, and removal. It MUST NOT know BibTeX syntax.
- **Import (`import.rs`)** parses a bibliography and converts entries to domain
  values. It reuses repository persistence rather than writing YAML directly.
- **Export (`export.rs`)** converts supplied domain records into BibLaTeX. It
  should not discover or walk repository directories itself.
- **Environmental boundaries (`launch.rs`)** isolate native viewing, editor
  invocation, and environment lookup for deterministic tests.

`ref` is deliberately small. Do not add service locators, dependency-injection
frameworks, event buses, plugin systems, actor systems, async runtimes, CQRS,
database abstraction layers, generic repository traits with one implementation,
or background services without strong evidence. Small targeted traits are right
for external effects—viewers, editors, processes, environment, time, randomness,
and future network providers. **Abstract nondeterministic or external effects,
not ordinary domain logic.**

Remain portable across macOS, Linux, and Windows where practical. Keep platform
behavior behind native abstractions and the domain platform-neutral. Linux CI is
not permission to add avoidable Unix-only assumptions.

## Import and Export Philosophy

### Import

`ref import bibliography.bib` parses the source before mutation where practical,
preserves citation keys, creates records without PDFs, reuses domain validation and
`Repository::add`, and never overwrites. The source file is never modified.

A structural/file-level parse failure aborts before repository mutation. Once the
file parses, a recoverable entry-level conversion or persistence failure skips that
entry, reports its key, and continues with later entries. An incomplete import
returns non-success. This best-effort entry behavior is intentional; do not turn it
into an all-or-nothing transaction.

The current importer is a purpose-built parser for supported BibTeX/BibLaTeX
metadata. It ignores attachments and unsupported fields, does not implement
cross-reference inheritance or editors, and maps unknown entry types to `misc` with
a warning. Do not parse standardized bibliography syntax with regular expressions;
an established parser crate is reasonable if the supported syntax grows.

### Export

BibLaTeX is derived output. Export MUST be deterministic, preserve citation keys,
use stable key ordering, format authors and escaping correctly, preserve UTF-8 where
appropriate, and omit generated timestamps. stdout is the default, intentionally
supporting `ref export > references.bib`; `--output` writes a file atomically.
`.ref` remains authoritative regardless of where an export is stored.

## Error, Failure, and Integrity Semantics

Expected invalid input MUST return a user-facing error, not panic. Prefer
`error: reference \`rocchio1971\` does not exist` to leaked internals; include a
useful cause for contextual failures such as malformed YAML. Errors and warnings
go to stderr, failures return non-zero, partial batches advertise partial failure,
and diagnostics identify affected citation keys. Never rely on color alone.

`doctor` is the integrity boundary for human-editable storage. It validates the
repository version and layout and diagnoses invalid keys, missing/malformed YAML,
invalid domain metadata, malformed or duplicate DOIs, and broken `paper.pdf`
entries. A missing PDF is valid. Missing recommended fields such as authors or year
are warnings; `doctor --strict` turns warnings into failure for stronger CI policy.

## Testing Philosophy

> Tests protect public behavior, domain contracts, and data-safety invariants.
> They should not freeze internal implementation details.

Prefer `public interface → observable result` over private-call, call-order, or
internal-data-structure assertions. High-value behavior includes discovery and
nearest-repository selection, key/path validation, add and no-PDF persistence,
manual-edit visibility, search, rename/delete safety, deterministic BibLaTeX,
partial import failures, doctor diagnostics, and CLI output/status behavior.

The filesystem is part of the domain. Use real isolated `tempfile::TempDir`
repositories for normal filesystem behavior rather than mocks. Abstract it only
when deterministic failure injection genuinely requires it. Conversely, tests MUST
never launch Preview, another PDF viewer, Vim, or VS Code: use the `launch` traits
and assert the path/command that would be invoked.

Tests MUST run in parallel, avoid shared global mutable state, network access,
sleeps, uncontrolled randomness, and assumptions about user configuration, and
work in headless CI. **A test that fails probabilistically is a bug in the test
suite.**

## Development Workflow

Required verification mirrors `.github/workflows/ci.yml`:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

Useful local entry points are:

```bash
cargo run -- --help
cargo run -- init
```

Agents making changes SHOULD:

1. Read this file and the relevant README sections.
2. Inspect the relevant modules and existing tests.
3. Run current tests before modifying code.
4. Identify the public behavior being changed.
5. Preserve repository and data-safety invariants.
6. Add or update behavioral tests.
7. Make the smallest coherent production change.
8. Run formatting, Clippy, and the full test suite.
9. Update Clap help and README when user-facing behavior changes.
10. Report any meaningful architectural deviation.

Do not update this constitution for every implementation detail. Update it when a
core invariant, major command, storage or architecture rule, roadmap/non-goal, or
material failure semantic changes.

## Rules for Changes

### Decision priority

When goals conflict, generally prefer: (1) data safety, (2) predictable behavior,
(3) repository compatibility, (4) human-readable state, (5) Git-friendly behavior,
(6) CLI ergonomics, (7) scriptability, (8) implementation simplicity,
(9) extensibility, then (10) performance.

### Evaluating new features

A proposed feature is a good fit when it:

1. improves reference management for scientific writing;
2. works naturally from a terminal;
3. preserves transparent project-local state;
4. is understandable without a background service;
5. composes with Git and shell tools; and
6. does not require users to surrender control of existing repositories.

Before implementation, ask:

- **New persistent state?** Can it be readable? Is it authoritative or derived?
  Can it be reconstructed, and could it create competing truths?
- **New configuration?** Prefer a strong convention unless real use cases need it.
- **Index or database?** Profile first; scanning YAML is expected to be sufficient.
- **Network?** Keep it optional and isolated; the core MUST remain useful offline.
- **Citation identity mutation?** Require explicit user intent.
- **Risk of data loss?** Design atomicity, recovery semantics, and tests first.
- **Niche workflow?** Prefer a small optional extension over complexity for all.

Give extra scrutiny to opaque state, mandatory connectivity, daemons, automatic
identity changes, large infrastructure, or synchronization between truths.

### Dependencies and implementation

Keep the dependency graph small, but use established crates for genuinely complex
external formats or platform behavior. CLI parsing, Serde/YAML, bibliography
parsing, temporary files, and native opening are reasonable areas. Before adding a
crate, ask whether the standard library is clear enough, whether the crate is
maintained, and whether it drags in an unnecessary runtime ecosystem. Do not
reimplement a complex standard merely to avoid one sensible dependency.

Prioritize correctness, data safety, clarity, maintainability, CLI ergonomics, and
only then performance. Do not add complexity to save imperceptible milliseconds.

## Roadmap

Roadmap items are direction, not implemented promises.

### Now / MVP

The current implementation supports initialization; PDF and no-PDF add with flags
or prompts; list, search, and show; native open and editor-based edit; rename and
confirmed remove; deterministic BibLaTeX export; BibTeX/BibLaTeX import; doctor;
behavioral/unit tests; Linux CI; and user documentation. Stabilizing and polishing
these capabilities defines the usable first version.

### Next

- Optional DOI lookup (for example `ref add paper.pdf --doi ...`) through an
  isolated provider such as Crossref; offline use must remain intact.
- A small citation helper such as `ref cite rocchio1971`, optionally copying
  `\cite{rocchio1971}`.
- LaTeX consistency checking such as `ref check thesis.tex` for missing/malformed
  cited keys and perhaps unused references—never automatic citation rewriting.
- CSL JSON or RIS import/export only in response to concrete use cases.

### Later / exploratory

Possible ideas include arXiv/DOI metadata providers, attachment-import assistance,
richer validation, configurable key generation, structured editor metadata,
multiple attachments, full-text search, shell completions, and global/shared library
concepts. These are exploratory, not implicit requirements. Anything weakening
transparent local state deserves particular scrutiny.

### Explicitly not planned

A cloud account system, desktop GUI, collaboration server, annotation sync,
reference recommendation engine, and opaque authoritative database remain outside
the plan.

## Definition of a Good Change

A good change solves a concrete scientific-reference workflow problem, preserves
or strengthens invariants, keeps storage understandable, respects existing
domain/repository boundaries, includes behavioral regression coverage, has
predictable CLI and failure semantics, avoids needless infrastructure, passes all
verification, and updates user-facing documentation where necessary.

**The fact that something can be implemented does not automatically mean it
belongs in `ref`.** New features should strengthen the transparent Git-like model,
not gradually turn the project into a desktop reference manager with a CLI attached.
