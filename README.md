# ref

`ref` is a small, Git-like reference manager for scientific writing. The filesystem is its database: each citation has readable YAML metadata and, optionally, one PDF. There is no index, remote service, or opaque database.

## Installation and quick start

Install stable Rust, then run `cargo install --path .` from this checkout.

```sh
cd thesis
ref init
ref add papers/rocchio.pdf
ref search rocchio
ref open rocchio1971
ref export > references.bib
```

Use the result in LaTeX with `\addbibresource{references.bib}`.

## Repository format

```text
.ref/
├── config.yaml
└── refs/
    └── rocchio1971/
        ├── ref.yaml
        └── paper.pdf
```

The directory is the citation key; the YAML deliberately has no duplicate `key` field:

```yaml
type: inproceedings
title: Relevance Feedback in Information Retrieval
authors:
  - given: Joseph J.
    family: Rocchio
year: 1971
container_title: The SMART Retrieval System
publisher: Prentice Hall
pages: 313-323
tags:
  - information-retrieval
```

Manual edits and copied reference directories are immediately visible. Commit `.ref` to Git if desired; `ref` never decides whether PDFs belong in version control.

## Commands

| Command | Purpose |
|---|---|
| `ref init` | Initialize `.ref` in the current directory |
| `ref add FILE [flags]` / `ref add --no-pdf` / `ref add --doi DOI` | Add metadata from flags, a PDF, or a DOI |
| `ref list [--sort key|year|author|title]` | List the collection |
| `ref search QUERY [--key|--author|--title|--year|--tag]` | Search metadata |
| `ref show KEY` | Show one exact key |
| `ref open KEY` | Open its PDF with the native viewer |
| `ref edit KEY` | Edit YAML using `$VISUAL`, then `$EDITOR` |
| `ref rename OLD NEW` | Safely rename the reference directory |
| `ref remove KEY [--yes]` (`ref rm`) | Remove the entire reference |
| `ref clean [PATH ...] [--dry-run] [--yes]` | Remove references unused in the current-directory scope |
| `ref export [biblatex] [--output FILE]` | Produce deterministic UTF-8 BibLaTeX |
| `ref import FILE` | Import a BibTeX/BibLaTeX bibliography without PDFs |
| `ref doctor [--strict]` | Validate structure, metadata, DOI syntax, and duplicates |

Run `ref COMMAND --help` for flags. Non-interactive adds require `--title`; `--key` is optional.

`ref add` prompts for missing title, authors, publication year, and citation key when run
interactively. Primary metadata can instead be provided directly (quote author values that
contain spaces):

```sh
ref add ~/Downloads/paper.pdf \
  --title "Relevance Feedback in Information Retrieval" \
  --author "Joseph J., Rocchio" \
  --year 1971

ref add paper.pdf \
  --title "Example Paper" \
  --author "Jane, Smith" \
  --author "John, Doe" \
  --year 2024

ref add --no-pdf \
  --title "Online Reference" \
  --author "Jane, Smith" \
  --year 2024

# Retrieve CSL-JSON metadata through doi.org; no PDF is downloaded.
ref add --doi 10.1038/nrd842
```

Each repeatable `--author` uses `Given names, Family name`; `John,Doe` is also
accepted.

### Citation keys

When no explicit `--key` is supplied, `ref` generates a key once, when the
reference is created, from the first author's complete family name, publication
year, and up to two title words. It selects the first two words that begin with
capital letters, falling back to the first two words when fewer than two are
capitalized. For example, Joseph J. Rocchio, 1971, *Relevance Feedback in
Information Retrieval* becomes `Rocchio1971RelevanceFeedback`.

Generated components are portable ASCII: multi-part names become PascalCase,
common Latin diacritics are transliterated, punctuation is removed, and uppercase
acronyms such as `EEG` are retained. A year and first author are required for
automatic generation; references lacking either can still use an explicit key.
Collisions receive uppercase alphabetic suffixes (`...`, `...A`, `...B`).

This convention is not a validity requirement. Explicit keys and bibliography
keys supplied to `ref import` are preserved exactly, and existing keys are never
regenerated when metadata is edited. Identity changes only through `ref rename`.

DOI lookup requires network access and a `curl` executable, and creates a reference
without a managed PDF.
Metadata is requested from `doi.org` using CSL-JSON content negotiation, then stored
in the same human-readable `.ref/refs/<key>/ref.yaml` file as every other reference.
You can inspect or edit that YAML normally after the lookup.

## Importing an existing bibliography

To migrate an existing LaTeX project, initialize a repository beside the document and
import its bibliography:

```sh
cd existing-thesis                 # contains thesis.tex and references.bib
ref init
ref import references.bib
```

Bibliography citation keys are preserved, so existing `\cite{...}` commands continue
to work. Each successful entry is stored using the normal repository format at
`.ref/refs/<key>/ref.yaml`. PDF and attachment fields are intentionally ignored; import
never copies or creates `paper.pdf`.

Import never overwrites an existing key. Conflicting entries are skipped, invalid
individual entries are reported, and processing continues, so an import may partially
succeed (with a non-zero status). A malformed bibliography is parsed before any files
are created and therefore does not partially mutate the repository. The source `.bib`
file is never modified.

## Removing unused references

Preview cleanup before removing metadata or attached PDFs:

```sh
ref clean --dry-run
ref clean                 # asks for confirmation
ref clean --yes           # explicitly skip confirmation
```

`clean` is deliberately directory-aware. Although repository discovery searches
upward for the nearest `.ref`, usage scanning starts at the invocation directory
and proceeds only downward. Paths further restrict that scope and are resolved
relative to the invocation directory:

```sh
cd chapters/eeg
ref clean --dry-run
ref clean -n sections/methods.tex notes.md
```

The summary makes a nested scope explicit because references used in parent or
sibling directories are not considered. Citation-key occurrences are literal,
case-sensitive, and token-bounded; comments and plain-text notes count as usage
to favor retaining a reference. `.bib`, `.bibtex`, PDF, binary, `.ref`, and `.git`
content is excluded. Common repository-root `.gitignore` and `.git/info/exclude`
rules (literal paths/directories, `*.extension`, and negation) are respected;
full Git glob syntax and nested ignore files are not currently interpreted. An
unreadable or invalid UTF-8 candidate text file makes the scan incomplete and prevents all deletion.
Use `--dry-run` before destructive cleanup, especially from a subdirectory.

## Design and limitations

Data safety, predictable behavior, readable YAML, and useful Git diffs take precedence over features. Writes use temporary paths and rename. Collection operations scan metadata, which is intentionally appropriate for thesis-sized libraries.

The MVP has no GUI, cloud sync, database, PDF discovery, attachment import, annotation handling, full-text indexing, citation insertion, or global library. It supports DOI metadata lookup, one optional PDF, local BibTeX/BibLaTeX metadata import, and one BibLaTeX exporter per reference. Cross-reference inheritance, editors, and unsupported fields are not imported; unknown entry types fall back to `misc`.
