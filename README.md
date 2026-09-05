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
| `ref add FILE [flags]` / `ref add --no-pdf` | Add metadata, optionally with a copied PDF |
| `ref list [--sort key|year|author|title]` | List the collection |
| `ref search QUERY [--key|--author|--title|--year|--tag]` | Search metadata |
| `ref show KEY` | Show one exact key |
| `ref open KEY` | Open its PDF with the native viewer |
| `ref edit KEY` | Edit YAML using `$VISUAL`, then `$EDITOR` |
| `ref rename OLD NEW` | Safely rename the reference directory |
| `ref remove KEY [--yes]` (`ref rm`) | Remove the entire reference |
| `ref export [biblatex] [--output FILE]` | Produce deterministic UTF-8 BibLaTeX |
| `ref doctor [--strict]` | Validate structure, metadata, DOI syntax, and duplicates |

Run `ref COMMAND --help` for flags. Non-interactive adds require `--key` and `--title`; authors use `--author 'Given|Family'` and may be repeated.

## Design and limitations

Data safety, predictable behavior, readable YAML, and useful Git diffs take precedence over features. Writes use temporary paths and rename. Collection operations scan metadata, which is intentionally appropriate for thesis-sized libraries.

The MVP has no GUI, cloud sync, database, metadata lookup, import, annotation handling, full-text indexing, citation insertion, or global library. It supports one optional PDF and one BibLaTeX exporter per reference.
