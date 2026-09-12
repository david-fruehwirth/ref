# Requirements

This document is the central specification of observable behavior for `ref`.
Requirement identifiers are permanent: add new identifiers rather than reusing or
renumbering existing ones. Comments on the test suite identify executable evidence
for these requirements; a requirement without such a comment records a known test
coverage gap.

## Repository

### REQ-001: Repository initialization

`ref init` shall create a repository with a versioned `.ref/config.yaml` file and a
`.ref/refs` directory, and shall refuse to initialize an existing repository.

### REQ-002: Repository discovery

Commands shall discover a repository by walking upward from the invocation
directory, selecting the nearest repository when repositories are nested.

### REQ-003: Missing repository

Repository-dependent commands shall fail when no `.ref` repository can be
discovered.

### REQ-004: Filesystem authority

The application shall read reference metadata from
`.ref/refs/<key>/ref.yaml` on each operation, observe valid manual edits, and derive
the citation key from the reference directory name.

### REQ-005: Safe citation keys

Citation keys shall be non-empty portable path components and shall reject path
traversal, separators, and whitespace.

### REQ-006: Reference key uniqueness

Each reference in a `.ref` repository shall have a unique citation key; creation
and import shall not overwrite an existing reference.

### REQ-007: Atomic reference creation

A failed reference creation or PDF copy shall leave neither a final reference nor
a staging directory that appears to be a valid reference.

## Reference metadata

### REQ-008: Human-readable metadata

Reference metadata, including Unicode and optional bibliographic fields, shall be
stored in human-readable YAML and shall round-trip without loss.

### REQ-009: Valid reference types

Stored metadata shall accept defined reference types and reject unknown type names.

### REQ-010: Structured authors

Authors shall retain structured given and family names in source order, and public
summaries shall handle zero, one, and multiple authors consistently.

### REQ-011: DOI normalization and validation

The application shall normalize common DOI URL and prefix forms to a canonical DOI
and shall reject malformed DOI values.

### REQ-012: Generated citation keys

Default citation keys shall be generated deterministically from the normalized
first-author family name, year, and selected title words, with uppercase
alphabetic suffixes used to resolve collisions.

### REQ-013: Explicit citation keys

Automatic key generation shall require an author and year, while a valid explicit
key shall remain usable when either value is absent.

## Add and attach

### REQ-014: Exclusive add sources

An add operation shall accept exactly one of a PDF, `--no-pdf`, or DOI lookup and
shall reject competing source modes.

### REQ-015: PDF copying

Adding a PDF shall copy it to the canonical `paper.pdf` location without modifying
the source file.

### REQ-016: References without PDFs

The application shall allow a reference to be added without a PDF when requested
and shall persist supplied valid metadata.

### REQ-017: DOI add

Adding from a DOI shall resolve CSL JSON metadata through the configured provider
and shall use the ordinary validated no-PDF persistence path.

### REQ-018: Failed add safety

Invalid input, failed DOI retrieval, a missing PDF, or a duplicate key shall cause
add to fail without leaving partial reference state.

### REQ-019: Attachment safety

`ref attach` shall copy a PDF to an existing reference, refuse missing references
and existing attachments, and resolve the missing-source warning after success.

### REQ-020: Interactive year resolution

Interactive add shall not prompt for a publication year when a year was supplied.

## List, search, and show

### REQ-021: Listing

`ref list` shall report stored references through the common output interface.
With no filter it shall list every reference; `--all` shall explicitly select the
same set.

### REQ-076: List usage filters

`ref list --used` shall include references whose citation keys occur in eligible
text files below the invocation directory, and `--unused` shall include the
complement. Detection, token boundaries, downward scope, ignored paths, supported
files, and incomplete-scan safety shall be identical to `ref clean`.

### REQ-077: List source-PDF filters

`ref list --pdf` shall include only references whose canonical `paper.pdf` is a
readable, non-empty regular file according to the repository source-proof check.
`--no-pdf` shall include its complement, including missing, unreadable, empty, and
non-file canonical paths.

### REQ-078: List filter composition and conflicts

One usage filter and one PDF filter may be combined, with logical AND semantics.
`--used` and `--unused` are mutually exclusive; `--pdf` and `--no-pdf` are
mutually exclusive; and `--all` cannot be combined with another filter. Invalid
combinations shall fail during argument parsing with an actionable diagnostic.

### REQ-079: Filtered list presentation

Filtering shall not change the established human reference-summary format or the
versioned list JSON schema. JSON shall contain no ANSI formatting. Empty human
results shall produce no reference blocks, and empty JSON results shall contain a
zero `reference_count` and an empty `references` array.

### REQ-022: Searching

`ref search` shall find matching stored bibliographic metadata and report results
through the common output interface.

### REQ-023: Exact show

`ref show` shall display the reference identified by an exact citation key,
including whether a PDF is present, and shall fail for a missing key.

## Open and edit

### REQ-024: Opening source PDFs

`ref open` shall ask the platform opener to open the canonical PDF of the exact
requested reference.

### REQ-025: Open preconditions

`ref open` shall not invoke a platform opener when the reference or its PDF is
missing, and shall report opener failures.

### REQ-026: Editor selection

`ref edit` shall open the exact reference metadata with `$VISUAL` in preference to
`$EDITOR`, without mutating process-global environment state.

### REQ-027: Edit safety

`ref edit` shall require a configured editor and shall preserve a user's edited
file while reporting metadata that is invalid after the editor exits.

## Rename and remove

### REQ-028: Exact mutation identity

Rename and removal shall require valid, exact citation keys and shall reject
missing sources and unsafe destination collisions.

### REQ-029: Rename preservation

Renaming shall move the complete reference directory to the new citation key
without changing its metadata or attachment contents.

### REQ-030: Confirmed removal

Removal shall delete the complete exact reference only after explicit confirmation
or `--yes`; declining confirmation shall preserve it.

## Clean

### REQ-031: Conservative usage matching

Clean shall treat citation keys as used only at token identity boundaries and
shall preserve any reference with such a textual occurrence.

### REQ-032: Clean scan exclusions

Clean shall exclude derived bibliographies, `.ref`, `.git`, PDFs, binary files,
and files covered by supported Git ignore rules from citation usage discovery.

### REQ-033: Downward clean scope

Clean shall scan downward from the invocation directory and shall never search its
parents or siblings, even when the repository was discovered above it.

### REQ-034: Optional clean paths

Explicit clean paths shall form a union of narrower scopes and shall be rejected
if they escape the invocation directory.

### REQ-035: Clean execution modes

`--dry-run` shall take precedence over `--yes`; destructive clean shall remove
only unused references and shall support confirmation or cancellation.

### REQ-036: Empty clean scope

Clean shall prominently report when its scope contains no eligible text files.

## Import

### REQ-037: Bibliographic field import

Import shall preserve citation keys and convert supported common fields, structured
authors, Unicode, keywords, notes, DOI values, page ranges, and preferred container
titles into reference metadata.

### REQ-038: Import without attachments

Imported bibliography entries shall create references without PDFs and shall
ignore source attachment fields.

### REQ-039: Import dates

Import shall derive a publication year from a valid bibliography `date` field when
an explicit year is absent.

### REQ-040: Parse-before-mutation

A malformed bibliography shall fail with line and column context before any
reference is imported.

### REQ-041: Best-effort entry import

After successful file parsing, invalid entries and conflicts shall be diagnosed
per entry, valid later entries shall still be imported, and an incomplete import
shall return non-success.

### REQ-042: Import collision safety

Import shall neither overwrite an existing citation key nor silently rename a
conflicting or duplicated source key.

### REQ-043: Import and export CLI status

The import CLI shall summarize successful imports and advertise partial failure;
successfully imported references shall be immediately available to show, search,
and export.

## BibTeX and BibLaTeX entry types

### REQ-044: Supported standard entry types

Import shall recognize the supported standard BibLaTeX/Biber entry types:
`article`, `book`, `mvbook`, `inbook`, `bookinbook`, `suppbook`, `booklet`,
`collection`, `mvcollection`, `incollection`, `suppcollection`, `dataset`,
`manual`, `misc`, `online`, `patent`, `periodical`, `suppperiodical`,
`proceedings`, `mvproceedings`, `inproceedings`, `reference`, `mvreference`,
`inreference`, `report`, `set`, `software`, `thesis`, `unpublished`, and `xdata`.

### REQ-045: Entry-type alias normalization

Import shall normalize `conference` to `inproceedings`; `electronic` and `www` to
`online`; `mastersthesis` and `phdthesis` to `thesis`; and `techreport` to
`report`.

### REQ-046: Case-insensitive entry types

BibTeX and BibLaTeX entry-type matching, including alias matching, shall be
case-insensitive.

### REQ-047: Unknown entry types

An unknown but syntactically valid bibliography entry type shall be imported as
`misc` with a warning that identifies the unsupported source type.

### REQ-048: Comment entries

`@comment` entries shall be ignored, including when the directive uses different
letter casing, and shall not create references or warnings.

### REQ-049: BibTeX strings and bracing

The bibliography parser shall expand defined string macros and preserve the text
of balanced nested braces while converting supported fields.

## Export

### REQ-050: Deterministic BibLaTeX export

BibLaTeX export shall be deterministic and ordered by citation key.

### REQ-051: BibLaTeX rendering

Export shall preserve citation keys and Unicode, correctly render structured
authors and supported fields, and escape BibLaTeX-sensitive text.

### REQ-052: Page rendering

Export shall normalize numeric page ranges for BibLaTeX while preserving
non-numeric page text rather than blindly rewriting it.

## Doctor

### REQ-053: Source-proof diagnostics

Doctor shall warn when a reference has no PDF and shall report an empty or
non-file canonical `paper.pdf` as an error.

### REQ-054: Structural and metadata diagnostics

Doctor shall diagnose invalid repository layout, invalid citation-key directories,
missing or malformed YAML, and invalid domain metadata.

### REQ-055: DOI diagnostics

Doctor shall distinguish malformed DOI values from duplicate normalized DOI values.

### REQ-056: Doctor policy and status

Doctor shall return success for a healthy repository, distinguish warnings from
errors in its counts and exit status, and make warnings fail under `--strict`.

## DOI metadata

### REQ-057: CSL conversion

DOI lookup shall convert complete CSL article metadata, structured contributors,
and valid issued dates into the reference metadata model.

### REQ-058: CSL type mapping

CSL conversion shall map supported publication types and use the documented
fallback for unknown CSL types.

### REQ-059: Invalid CSL responses

DOI lookup shall reject responses missing required bibliographic fields or
containing malformed publication dates.

### REQ-060: DOI request format

The DOI provider shall request CSL JSON and parse a successful CSL JSON response.

### REQ-061: DOI failure categories

DOI lookup shall expose distinguishable not-found, rate-limit, server, malformed
response, and transport/retrieval failures.

### REQ-062: DOI timeout

DOI lookup shall apply its configured timeout and report timeout as a retrieval
failure.

### REQ-063: DOI redirects

DOI lookup shall follow HTTP redirects to a successful metadata response.

## CLI and output behavior

### REQ-064: Stable command help

Top-level help, version output, and core subcommand help shall expose the supported
command surface, options, and relevant safety semantics.

### REQ-065: End-to-end command composition

Initialization, add, list, search, show, rename, remove, import, and export shall
compose through their documented CLI workflows without hidden state.

### REQ-066: Output and exit conventions

Normal results shall be written to stdout, diagnostics to stderr, and command
failures or partial failures shall return a non-zero status.

### REQ-067: JSON envelope

JSON list and search results shall use the common versioned success envelope;
structured failures shall be written only to stderr with an error code and
relevant context.

### REQ-068: Non-interactive JSON removal

JSON-mode removal shall require explicit `--yes` confirmation rather than prompt.

## Recently added references

### REQ-069: Persisted reference creation time

Every successfully created reference shall store an unambiguous UTC `added_at`
timestamp in its versioned `ref.yaml` state through the shared repository creation
path. Existing references without this property shall remain valid, and rename or
other non-creation operations shall preserve it.

### REQ-070: Recently added query

`ref last` shall return up to the requested positive number of references having
persisted creation metadata, ordered by `added_at` descending and citation key
ascending for ties. It shall exclude legacy references without `added_at`, and an
empty result shall succeed silently.

### REQ-071: Plain last output

Human-readable `ref last` output shall contain exactly one citation key per line,
without headers, labels, timestamps, or explanatory text.

### REQ-072: Structured last output

JSON `ref last` output shall use the common versioned success envelope and expose
the selected references as objects containing `citation_key`, without exposing
their creation timestamps.

### REQ-073: Human reference summaries

Human-readable `ref list` and `ref search` results shall use one shared reference
summary representation. Each reference shall be a separate block headed by its
citation key, followed by indented, consistently aligned labels for its title,
authors when present, publication year when present, and reference type. Blocks
shall be separated by one blank line and shall not use a column-oriented table or
depend on terminal-width calculations. Unicode values shall be preserved.

### REQ-074: Summary color and redirection

The shared human reference summary may style citation keys and labels when stdout
is an interactive terminal and color is enabled. It shall emit no terminal control
sequences when stdout is redirected or piped, or when `NO_COLOR` is present, so
plain results remain suitable for stdout redirection.

### REQ-075: Human and JSON presentation separation

Human reference-summary formatting and color shall not alter list or search JSON
output, which shall continue to use the existing schema, fields, values, and
versioned envelope without terminal control sequences.
