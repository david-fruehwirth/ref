# ref

`ref` is a Git-like, CLI-native reference manager for scientific writing. References live inside a project-local `.ref` directory as human-readable YAML metadata with optional PDFs. The filesystem is the database, so a library remains easy to inspect, edit, diff, and version beside a LaTeX thesis or paper.

The numbered application behavior specification is maintained in
[`docs/requirements.md`](docs/requirements.md); test annotations link existing
tests to the requirements they validate.

## Why `ref`?

`ref` is designed for terminal-centric, project-local writing workflows. It favors readable files and explicit commands over an opaque global database: metadata can be reviewed in Git, source PDFs can live beside it, and BibLaTeX can be generated whenever the document is built. Normal use requires no GUI, account, background service, or network connection; only optional DOI metadata lookup uses the network.

## Installation

Install with Homebrew:

```bash
brew install david-fruehwirth/ref/ref
```

As a fallback, install stable Rust, clone this repository, and install the binary from the checkout:

```bash
cargo install --path .
```

## Quick start

```bash
mkdir thesis
cd thesis
ref init

ref add ~/Downloads/rocchio.pdf \
  --title "Relevance Feedback in Information Retrieval" \
  --author "Joseph J., Rocchio" \
  --year 1971

ref list
ref search rocchio
ref last
ref open Rocchio1971RelevanceFeedback
ref export > references.bib
ref doctor
```

Use the generated bibliography from LaTeX with, for example, `\addbibresource{references.bib}`.

## How it works

### Repository structure

`ref init` creates `.ref/config.yaml` and `.ref/refs/` in the current directory. It does not initialize Git. A populated project looks like this:

```text
thesis/
├── .git/
├── .ref/
│   ├── config.yaml
│   └── refs/
│       ├── Rocchio1971RelevanceFeedback/
│       │   ├── ref.yaml
│       │   └── paper.pdf
│       └── Smith2024AttentionModels/
│           └── ref.yaml
├── thesis.tex
└── references.bib
```

The reference directory name is its citation key and canonical identity. `ref.yaml` is authoritative bibliographic metadata; `paper.pdf` is the optional canonical source attachment. A representative metadata file is:

```yaml
type: article
title: Example Paper
authors:
  - given: Jane
    family: Smith
year: 2024
container_title: Journal of Examples
publisher: Example Press
volume: "12"
issue: "3"
pages: 10-20
doi: 10.1234/example
url: https://example.org
tags:
  - recommender-systems
notes: |
  Relevant to the methodology section.
```

These field names are part of the stored format. Manual editing of `.ref/refs/<key>/ref.yaml` is supported, either directly or with `ref edit <key>`; run `ref doctor` afterward to detect many mistakes. Use `ref rename` rather than manually renaming a reference directory.

### Repository discovery

Except for `init`, commands find the nearest `.ref` directory by searching the current directory and then its parents. For example, running this from `thesis/chapters/methods`:

```bash
ref search rocchio
```

finds `thesis/.ref`. If repositories are nested, the nearest one wins.

`clean` is intentionally asymmetric: it also discovers `.ref` upward, but searches for citation usage only from the invocation directory downward. See [`ref clean`](#ref-clean) before using it from a subdirectory.

### Citation keys

When `--key` is omitted, a newly added reference gets a creation-time key of the general form:

```text
<FirstAuthorFamilyName><Year><UpToTwoTitleWords>
```

`ref` normalizes the first author's complete family name, then prefers the first two title words beginning with uppercase letters. If fewer than two title words are capitalized, it uses the first two usable words instead. For example:

```text
Author: Joseph J. Rocchio
Year:   1971
Title:  Relevance Feedback in Information Retrieval
Key:    Rocchio1971RelevanceFeedback
```

Multi-part surnames become PascalCase (`van der Waals` becomes `VanDerWaals`), common Latin diacritics are transliterated (`Müller` becomes `Muller`), and punctuation is removed (`O'Connor` becomes `OConnor`). Collisions receive uppercase alphabetic suffixes such as `A` and `B`.

Automatic generation requires a first author and year. `--key` overrides generation, and imported bibliography keys are preserved. A key is stable after creation: editing title, author, or year never regenerates it. Change identity explicitly with `ref rename`, which does **not** rewrite citations in project source files.

### PDFs and source availability

A reference may exist without a PDF, especially after `ref import`, `ref add --no-pdf`, or DOI lookup. Add one later with `ref attach`. Both `add` and `attach` copy the PDF into the repository without deleting or changing the source, and neither silently overwrites an existing reference attachment.

`ref doctor` reports a missing source PDF as a warning. An empty or non-file `paper.pdf` is an error. `ref doctor --strict` is therefore useful as a final thesis-quality check requiring a canonical local source artifact for every reference.

A PDF indicates only local source availability. Its presence does not establish that it is the correct publication, was read, supports a claim, or is scientifically valid.

### Author syntax

Each author supplied to `add` uses `--author "Given names, Family name"`; repeat the option to preserve multiple authors in order:

```bash
ref add paper.pdf \
  --title "Attention Is All You Need" \
  --author "Ashish, Vaswani" \
  --author "Noam, Shazeer" \
  --year 2017
```

Quote values containing spaces so the shell passes each author as one argument. `--author John,Doe` also parses, but the quoted form is clearer.

## Command reference

### Machine-readable output

Every command that produces application output accepts the global `--json` flag,
either before or after the command name:

```bash
ref list --json
ref search attention --json
ref doctor --json
ref clean --dry-run --json
```

JSON output is intended for scripts, editors, CI, and AI tools. Successful results
are written as one pretty-printed JSON document to stdout; structured command
failures are written to stderr while stdout remains empty. JSON mode is always
non-interactive, so destructive commands require `--yes`. The top-level
`schema_version` versions this machine-output API independently of the repository
format version. For example, `jq` can select matching citation keys (it is not a
dependency of `ref`):

```bash
ref search attention --json |
  jq -r '.result.matching_references[].citation_key'
```

| Command                 | Purpose                                                |
| ----------------------- | ------------------------------------------------------ |
| `ref init`              | Initialize a `.ref` repository                         |
| `ref add`               | Add a reference with a PDF, without one, or from a DOI |
| `ref list`              | List stored references                                 |
| `ref search`            | Search bibliographic metadata                          |
| `ref last`              | Print recently added citation keys                     |
| `ref show`              | Show one exact citation key                            |
| `ref open`              | Open a reference's source PDF                          |
| `ref attach`            | Attach a PDF to an existing reference                  |
| `ref edit`              | Edit YAML metadata                                     |
| `ref rename`            | Change a citation key                                  |
| `ref remove` / `ref rm` | Remove a reference and its files                       |
| `ref clean`             | Find and remove unused references                      |
| `ref import`            | Import a BibTeX/BibLaTeX bibliography                  |
| `ref export`            | Generate BibLaTeX                                      |
| `ref doctor`            | Validate repository health and completeness            |
| `ref help`              | Show top-level or command-specific help                |

Run `ref <command> --help` (or `ref help <command>`) for complete argument and option help.

### `ref init`

Initialize a reference repository in the current directory:

```bash
ref init
```

It creates `.ref/config.yaml` and `.ref/refs/`; it does not create a Git repository.

### `ref add`

Add a reference and optionally copy a source PDF into it:

```bash
ref add [PDF] [OPTIONS]
```

With an interactive terminal, omitted title, authors, year, and generated citation key can be prompted for. A non-interactive metadata add requires `--title`. Important options include `--key`, repeatable `--author`, `--year`, `--type`, `--container-title`, `--publisher`, `--doi`, `--url`, comma-separated `--tags`, and `--no-pdf`.

```bash
# Interactive metadata entry with a PDF
ref add paper.pdf

# Fully specified PDF reference with multiple authors
ref add paper.pdf \
  --title "Example Paper" \
  --author "Jane, Smith" \
  --author "John, Doe" \
  --year 2024

# Bibliographic metadata without a PDF
ref add --no-pdf \
  --title "Online Reference" \
  --author "Jane, Smith" \
  --year 2024

# Override generated identity
ref add paper.pdf \
  --key customKey \
  --title "Example Paper" \
  --author "Jane, Smith" \
  --year 2024
```

`ref add --doi 10.1038/nrd842` retrieves CSL-JSON metadata through `doi.org`, creates a reference without a PDF, and requires network access plus a `curl` executable. No PDF is downloaded. If `--doi` is supplied alongside an explicit title, it is stored as manually entered metadata rather than used as the lookup source.

### `ref list`

List references as readable blocks headed by their citation keys. Sort by `key`
(the default), `year`, `author`, or `title`:

```bash
ref list
ref list --sort year
ref list --sort author
```

```text
Smith2024Attention
    Title:   Attention and Social Media
    Authors: John Smith, Jane Doe
    Year:    2024
    Type:    article
```

### `ref search`

Perform a case-insensitive substring search across citation keys, authors, titles, tags, and years. Results use deterministic relevance and citation-key ordering:

```bash
ref search <QUERY>
ref search rocchio
ref search "relevance feedback"
ref search smith --author
ref search attention --title
ref search 2024 --year
ref search eeg --tag
ref search Rocchio --key
```

Use at most one of `--key`, `--author`, `--title`, `--year`, or `--tag` per search.

### `ref last`

Print the citation key most recently added to the repository, or request more
keys in newest-first order. Output is one key per line for direct use in shell
pipelines. Legacy references without persisted creation metadata are omitted.

```bash
ref last
ref last -n 5
key=$(ref last)
```

### `ref show`

Show detailed metadata and PDF availability for one exact citation key:

```bash
ref show Rocchio1971RelevanceFeedback
```

### `ref open`

Open `.ref/refs/<key>/paper.pdf` in the operating system's default application:

```bash
ref open Rocchio1971RelevanceFeedback
```

The command requires an exact citation key and fails if no source PDF is attached.

### `ref attach`

Copy a non-empty PDF onto an existing reference that does not already have one:

```bash
ref attach Smith2024Attention ~/Downloads/paper.pdf
```

The canonical destination is `paper.pdf`. The source is untouched, and an existing destination is never overwritten.

### `ref edit`

Open a reference's `ref.yaml` using `$VISUAL`, falling back to `$EDITOR`:

```bash
ref edit Rocchio1971RelevanceFeedback
```

`ref` validates the metadata after the editor exits. If validation fails, it reports an error but preserves the edited file so it can be corrected.

### `ref rename`

Change a reference's citation key and directory name:

```bash
ref rename Smith2024OldTitle Smith2024BetterTitle
```

Both keys are exact. Existing keys are never overwritten. **Rename does not rewrite `\cite{...}` commands or any other project files**; update those uses yourself.

### `ref remove`

Delete the complete reference directory, including `ref.yaml`, `paper.pdf`, and any other files it contains:

```bash
ref remove Smith2024Attention
ref rm Smith2024Attention
ref remove Smith2024Attention --yes
```

The command requires confirmation. In non-interactive use, pass `--yes` explicitly.

### `ref clean`

Find citation keys with no textual occurrence in the selected source scope, then remove their complete reference directories:

```bash
ref clean [PATH ...] [--dry-run] [--yes]
ref clean --dry-run
ref clean
ref clean --yes
ref clean chapters/
ref clean introduction.tex chapters/methods/
```

The repository is discovered upward, but the **scan root is the current directory**, and scanning only moves downward. Optional paths are relative to that directory and can only narrow the scan. Running from a nested directory may classify references used in parent or sibling directories as unused, so preview with `--dry-run` first.

Matching is literal, case-sensitive, and bounded by citation-key characters. An exact occurrence in a comment or plain-text note counts as usage; this conservative rule favors retaining references. `.ref`, `.git`, PDFs, `.bib`, `.bibtex`, binary files, and files covered by the supported repository-root Git ignore rules do not count. If any candidate cannot be inspected completely, deletion is prevented. Without `--yes`, destructive cleanup asks for confirmation.

### `ref import`

Import BibTeX/BibLaTeX records as normal `.ref` references:

```bash
ref import references.bib
```

Citation keys are preserved, the source bibliography is not modified, and attachment fields/PDFs are not imported. Existing citation keys are never overwritten. A structural parse failure occurs before mutation. After successful parsing, invalid or conflicting individual entries are reported and skipped while later entries continue; a partial import exits non-zero.

For an existing thesis:

```bash
cd thesis
ref init
ref import references.bib
ref doctor
```

Existing `\cite{...}` uses retain their identities because import preserves keys. `doctor` will identify imported records whose source PDFs are missing.

### `ref export`

Generate deterministic UTF-8 BibLaTeX ordered by citation key:

```bash
ref export
ref export > references.bib
ref export --output references.bib
```

Only the `biblatex` format is currently supported (and is the default). `--output` writes atomically; stdout supports shell redirection. The authority model is:

```text
.ref/             authoritative reference data
references.bib    generated, derived representation
```

Do not maintain independent edits in generated `references.bib`; they are lost the next time it is exported.

### `ref doctor`

Check repository structure, citation keys, YAML/domain metadata, DOI syntax and duplicates, and source PDF status:

```bash
ref doctor
ref doctor --strict
```

Diagnostics have two severities:

- **Errors** indicate invalid structure or data, including malformed metadata/DOIs and invalid or empty source PDFs. They return a non-zero status.
- **Warnings** identify usable but incomplete or suspicious records, including missing authors, years, source PDFs, and duplicate normalized DOIs. A normal run still succeeds when it has only warnings.
- **Strict mode** makes warnings return a non-zero status too, which is useful in CI and before final submission.

## Typical thesis workflow

1. Add a source and its metadata:
   ```bash
   ref add paper.pdf --title "Attention Models" --author "Jane, Smith" --year 2024
   ```
2. Find and read it later:
   ```bash
   ref search attention
   ref open Smith2024AttentionModels
   ```
3. Cite the stable key in LaTeX:
   ```latex
   \cite{Smith2024AttentionModels}
   ```
4. Generate the bibliography before compilation:
   ```bash
   ref export > references.bib
   ```
5. Check routine quality, then apply a strict final gate:
   ```bash
   ref doctor
   ref doctor --strict
   ```
6. Review stale references conservatively:
   ```bash
   ref clean --dry-run
   ref clean
   ```

## Git integration

The `.ref` directory is designed to evolve alongside writing source:

```bash
git add .ref
git commit -m "Add references for attention section"
```

Metadata diffs are readable, citation identity is visible in paths, and no hidden database must be synchronized. Whether to commit `paper.pdf` files is a project decision; `ref` does not automatically add them to `.gitignore`.

## CI and validation

Once the `ref` binary is installed in CI, strict doctor can serve as a completeness gate:

```yaml
- name: Validate references
  run: ref doctor --strict
```

A document build may regenerate derived bibliography output first:

```bash
ref export > references.bib
```

## Design principles

The `.ref` filesystem is authoritative, repository operations favor explicit identity and data safety, and generated output is deterministic. Commands compose through stdout, stderr, and exit status. Linear scans are intentional for thesis-sized collections.

## Limitations and non-goals

`ref` manages one optional PDF per reference. It is not a GUI, cloud-sync service, background daemon, PDF reader, annotation manager, opaque database, OCR/full-text index, or claim-verification system. It does not rewrite LaTeX citation keys automatically. Import does not currently implement cross-reference inheritance or editors, and unsupported fields are ignored; unknown entry types are stored as `misc` with a warning.

## Development

Contributor architecture and safety guidance lives in [`AGENTS.md`](AGENTS.md). Run the same checks used by CI:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

Inspect the CLI locally with:

```bash
cargo run -- --help
cargo run -- <command> --help
```
