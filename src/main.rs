use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use dialoguer::{Confirm, Input};
use r#ref::{
    clean::{self, CleanAnalysis},
    doctor::{self, DiagnosticSeverity},
    export,
    import::{self, ImportResult},
    launch::{self, Editor, Environment, FileOpener},
    metadata::{Doi, DoiMetadataClient},
    model::{
        display_author, generated_key, validate_year, CitationKey, Person, Reference, ReferenceType,
    },
    repository::{Repository, StoredReference},
};
use std::{
    env, fs,
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

struct SystemOpener;
impl FileOpener for SystemOpener {
    fn open(&self, path: &Path) -> Result<()> {
        open::that_detached(path).map_err(Into::into)
    }
}
struct SystemEnvironment;
impl Environment for SystemEnvironment {
    fn variable(&self, name: &str) -> Option<std::ffi::OsString> {
        env::var_os(name)
    }
}
struct SystemEditor;
impl Editor for SystemEditor {
    fn edit(&self, command: &str, path: &Path) -> Result<bool> {
        let mut parts = command.split_whitespace();
        let program = parts.next().context("editor command is empty")?;
        Ok(Command::new(program)
            .args(parts)
            .arg(path)
            .status()?
            .success())
    }
}

#[derive(Parser)]
#[command(
    name = "ref",
    version,
    about = "A Git-like, project-local reference manager for scientific writing",
    long_about = "A Git-like, project-local reference manager for scientific writing.\n\nref stores human-readable metadata and optional source PDFs in a project-local .ref directory. Run commands anywhere inside the project; ref discovers the nearest repository by searching parent directories.",
    after_help = "Run `ref <COMMAND> --help` for details about a command."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a reference repository in the current directory
    #[command(
        long_about = "Initialize a new reference repository in the current directory.\n\nCreates:\n  .ref/config.yaml\n  .ref/refs/\n\nThis command does not initialize a Git repository.",
        after_help = "Example:\n  ref init"
    )]
    Init,
    /// Add a bibliographic reference, optionally with a source PDF
    #[command(
        long_about = "Add a bibliographic reference, optionally with a source PDF.\n\nThe PDF is copied into the repository; the source file is not changed. Without --key, ref generates a citation key from the first author's family name, publication year, and title. Missing metadata is prompted for only when stdin is interactive. Use --doi by itself to retrieve metadata, or --no-pdf to enter metadata without a PDF.",
        after_help = "Examples:\n  ref add paper.pdf\n  ref add paper.pdf --title \"Example Paper\" --author \"Jane, Smith\" --year 2024\n  ref add --no-pdf --title \"Example Paper\" --author \"Jane, Smith\" --year 2024\n  ref add --doi 10.1234/example"
    )]
    Add(AddArgs),
    /// List references in the repository
    #[command(
        after_help = "Examples:\n  ref list\n  ref list --sort year\n  ref list --sort author"
    )]
    List {
        /// Sort references by citation key, year, first author, or title
        #[arg(long, value_enum, default_value = "key")]
        sort: Sort,
    },
    /// Search references by key, title, author, year, or tags
    #[command(
        long_about = "Search references by citation key, title, author, year, or tags.\n\nMatching is case-insensitive substring search. Results are ordered by relevance and then citation key. Supply at most one field filter.",
        after_help = "Examples:\n  ref search rocchio\n  ref search \"relevance feedback\"\n  ref search smith --author\n  ref search attention --title\n  ref search eeg --tag"
    )]
    Search(SearchArgs),
    /// Show detailed metadata for a reference
    #[command(
        long_about = "Show detailed metadata for one reference identified by its exact citation key."
    )]
    Show {
        /// Exact citation key to show
        key: String,
    },
    /// Open a reference's source PDF in the system viewer
    #[command(
        long_about = "Open .ref/refs/<KEY>/paper.pdf in the operating system's default application. The command fails when the exact citation key does not exist or has no source PDF."
    )]
    Open {
        /// Exact citation key whose source PDF should be opened
        key: String,
    },
    /// Attach a source PDF to an existing reference
    #[command(
        long_about = "Attach a source PDF to an existing reference that has no attachment.\n\nThe PDF is copied to .ref/refs/<KEY>/paper.pdf. The source remains untouched, and an existing source PDF is never overwritten.",
        after_help = "Example:\n  ref attach Smith2024Attention ~/Downloads/paper.pdf"
    )]
    Attach {
        /// Exact citation key to receive the source PDF
        key: String,
        /// PDF file to copy into the reference repository
        pdf: PathBuf,
    },
    /// Edit a reference's YAML metadata in $VISUAL or $EDITOR
    #[command(
        long_about = "Edit a reference's ref.yaml metadata using $VISUAL, falling back to $EDITOR. The exact citation key and edited metadata are validated after the editor exits; invalid user edits are left in place for correction."
    )]
    Edit {
        /// Exact citation key to edit
        key: String,
    },
    /// Rename a reference's citation key
    #[command(
        long_about = "Rename a reference's citation key.\n\nThis changes the reference directory and repository identity. It does not rewrite citations in .tex or other project files.",
        after_help = "Example:\n  ref rename Smith2024OldTitle Smith2024BetterTitle"
    )]
    Rename {
        /// Existing exact citation key
        old_key: String,
        /// New citation key
        new_key: String,
    },
    /// Remove a reference and its attached files
    #[command(
        alias = "rm",
        long_about = "Remove a reference's entire directory, including ref.yaml, its source PDF, and any other attached files.\n\nThe exact citation key must be confirmed unless --yes is supplied. Non-interactive use requires --yes.",
        after_help = "Examples:\n  ref remove Smith2024Attention\n  ref rm Smith2024Attention\n  ref remove Smith2024Attention --yes"
    )]
    Remove {
        /// Exact citation key to remove
        key: String,
        /// Skip confirmation and remove the reference immediately
        #[arg(long, short)]
        yes: bool,
    },
    /// Find and remove references unused in the current source scope
    #[command(
        long_about = "Find and remove references unused in the current source scope.\n\nThe repository is discovered by searching parent directories, but citation usage is searched only from the current directory downward. Optional paths can narrow, but never expand, that scope. .ref, .git, PDFs, and .bib/.bibtex files are excluded. An incomplete scan prevents deletion. Preview with --dry-run before destructive use.",
        after_help = "Examples:\n  ref clean --dry-run\n  ref clean\n  ref clean --yes\n  ref clean chapters/\n  ref clean introduction.tex chapters/methods/"
    )]
    Clean(CleanArgs),
    /// Export references as deterministic BibLaTeX
    #[command(
        long_about = "Export repository metadata as deterministic BibLaTeX ordered by citation key.\n\nOutput is written to stdout by default. The .ref repository remains authoritative; exported bibliography files are derived output.",
        after_help = "Examples:\n  ref export\n  ref export > references.bib\n  ref export --output references.bib"
    )]
    Export {
        /// Export format (currently only biblatex)
        #[arg(default_value = "biblatex")]
        format: String,
        /// Write output atomically to this file instead of stdout
        #[arg(long, short)]
        output: Option<PathBuf>,
    },
    /// Import references from a BibTeX/BibLaTeX bibliography
    #[command(
        long_about = "Import references from a BibTeX/BibLaTeX bibliography.\n\nCitation keys are preserved, and PDFs are not imported. Existing references are never overwritten. After the file is parsed, invalid or conflicting entries are reported and skipped while later entries continue; any such partial import returns a non-zero status.",
        after_help = "Example:\n  ref import references.bib"
    )]
    Import {
        /// BibTeX or BibLaTeX bibliography file
        file: PathBuf,
    },
    /// Check repository integrity and source completeness
    #[command(
        long_about = "Check repository integrity and reference completeness.\n\nValidates repository structure, citation keys, metadata, DOI syntax and duplicates, and source PDFs. Missing authors, years, and source PDFs are warnings; malformed metadata and invalid or empty source PDFs are errors. Warnings do not fail normal doctor runs.",
        after_help = "Examples:\n  ref doctor\n  ref doctor --strict"
    )]
    Doctor {
        /// Treat warnings as failures, returning a non-zero exit status
        #[arg(long)]
        strict: bool,
    },
}
#[derive(Args)]
struct CleanArgs {
    /// Restrict scanning to files/directories below the current directory
    paths: Vec<PathBuf>,
    /// Show references that would be removed without modifying the repository
    #[arg(long, short = 'n')]
    dry_run: bool,
    /// Remove unused references without confirmation
    #[arg(long, short = 'y')]
    yes: bool,
}
#[derive(Args)]
struct AddArgs {
    /// PDF to attach (required unless --no-pdf or --doi is used)
    #[arg(required_unless_present_any = ["no_pdf", "doi"], conflicts_with_all = ["no_pdf", "doi"])]
    pdf: Option<PathBuf>,
    /// Create a reference without an attached PDF
    #[arg(long, conflicts_with = "pdf")]
    no_pdf: bool,
    /// Explicit citation key; otherwise generated from author, year, and title
    #[arg(long)]
    key: Option<String>,
    /// Publication title
    #[arg(long)]
    title: Option<String>,
    /// Author in `Given names, Family name` format; may be specified multiple times
    #[arg(long, value_parser=parse_author)]
    author: Vec<Person>,
    /// Publication year
    #[arg(long, value_parser=parse_year)]
    year: Option<u16>,
    /// Bibliographic entry type
    #[arg(long = "type", default_value = "article")]
    entry_type: ReferenceType,
    /// Journal, proceedings, or other containing publication title
    #[arg(long)]
    container_title: Option<String>,
    /// Publication's publisher
    #[arg(long)]
    publisher: Option<String>,
    /// Retrieve CSL-JSON metadata for this DOI (requires network access; creates no PDF)
    #[arg(long)]
    doi: Option<String>,
    /// Publication URL
    #[arg(long)]
    url: Option<String>,
    /// Comma-separated tags; the option may also be repeated
    #[arg(long, value_delimiter = ',')]
    tags: Vec<String>,
}
#[derive(Args)]
struct SearchArgs {
    /// Case-insensitive substring to find
    query: String,
    /// Search authors only
    #[arg(long)]
    author: bool,
    /// Search titles only
    #[arg(long)]
    title: bool,
    /// Search publication years only
    #[arg(long)]
    year: bool,
    /// Search tags only
    #[arg(long)]
    tag: bool,
    /// Search citation keys only
    #[arg(long)]
    key: bool,
}
#[derive(Clone, ValueEnum)]
enum Sort {
    Key,
    Year,
    Author,
    Title,
}

fn parse_author(s: &str) -> std::result::Result<Person, String> {
    let hint =
        || format!("invalid author `{s}`; use `Given names, Family name`, for example `John, Doe`");
    let (given, family) = s.split_once(',').ok_or_else(hint)?;
    let given = given.trim();
    let family = family.trim();
    if given.is_empty() || family.is_empty() {
        return Err(hint());
    }
    Ok(Person {
        given: given.to_owned(),
        family: family.to_owned(),
    })
}

fn parse_year(s: &str) -> std::result::Result<u16, String> {
    let year = s
        .parse::<u16>()
        .map_err(|_| format!("invalid publication year `{s}`"))?;
    validate_year(year).map_err(|error| error.to_string())?;
    Ok(year)
}

fn resolve_year(
    supplied: Option<u16>,
    interactive: bool,
    mut prompt: impl FnMut() -> Result<String>,
) -> Result<Option<u16>> {
    if supplied.is_some() || !interactive {
        return Ok(supplied);
    }
    loop {
        let value = prompt()?;
        if value.trim().is_empty() {
            return Ok(None);
        }
        match parse_year(value.trim()) {
            Ok(year) => return Ok(Some(year)),
            Err(error) => eprintln!("Invalid year: {error}. Please try again."),
        }
    }
}
fn repo() -> Result<Repository> {
    Repository::discover(&env::current_dir()?)
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(1)
        }
    }
}
fn run() -> Result<u8> {
    match Cli::parse().command {
        Commands::Init => {
            let r = Repository::init(&env::current_dir()?)?;
            println!("Initialized reference repository at {}", r.root().display());
        }
        Commands::Add(a) => add(&repo()?, a)?,
        Commands::List { sort } => list(&repo()?, sort)?,
        Commands::Search(a) => search(&repo()?, a)?,
        Commands::Show { key } => show(&repo()?.load_reference(&CitationKey::new(key)?)?),
        Commands::Open { key } => open_pdf(&repo()?, key)?,
        Commands::Attach { key, pdf } => {
            let key = CitationKey::new(key)?;
            repo()?.attach(&key, &pdf)?;
            println!("Attached source PDF to {key}");
        }
        Commands::Edit { key } => edit(&repo()?, key)?,
        Commands::Rename { old_key, new_key } => {
            let old = CitationKey::new(old_key)?;
            let new = CitationKey::new(new_key)?;
            repo()?.rename(&old, &new)?;
            println!("Renamed {old} → {new}\n\nNote: existing \\cite{{{old}}} references are not updated automatically.");
        }
        Commands::Remove { key, yes } => remove(&repo()?, key, yes)?,
        Commands::Clean(args) => return clean_cmd(&repo()?, args),
        Commands::Export { format, output } => export_cmd(&repo()?, &format, output.as_deref())?,
        Commands::Import { file } => return import_cmd(&repo()?, &file),
        Commands::Doctor { strict } => return doctor(&repo()?, strict),
    };
    Ok(0)
}

fn import_cmd(repo: &Repository, path: &Path) -> Result<u8> {
    if !path.is_file() {
        bail!(
            "bibliography `{}` does not exist or is not a regular file",
            path.display()
        );
    }
    let source =
        fs::read_to_string(path).with_context(|| format!("failed to read `{}`", path.display()))?;
    let entries = import::parse_bibliography(&source)
        .with_context(|| format!("failed to parse `{}`", path.display()))?;
    let result = import::import_bibliography(repo, entries);
    print_import_diagnostics(&result);
    if result.is_complete() {
        println!(
            "Imported {} references from {}",
            result.imported,
            path.display()
        );
    } else {
        println!(
            "Import complete with errors.\n\nEntries:  {}\nImported: {}\nSkipped:  {}\nFailed:   {}",
            result.total,
            result.imported,
            result.skipped.len(),
            result.failed.len()
        );
    }
    Ok(u8::from(!result.is_complete()))
}

fn print_import_diagnostics(result: &ImportResult) {
    for diagnostic in &result.skipped {
        eprintln!(
            "warning: skipped `{}`\n  {}",
            diagnostic.key, diagnostic.message
        );
    }
    for diagnostic in &result.failed {
        eprintln!(
            "warning: failed to import `{}`\n  {}",
            diagnostic.key, diagnostic.message
        );
    }
    for diagnostic in &result.warnings {
        eprintln!("warning: `{}`: {}", diagnostic.key, diagnostic.message);
    }
}

fn add(repo: &Repository, mut a: AddArgs) -> Result<()> {
    if a.no_pdf && a.pdf.is_some() {
        bail!("a PDF and --no-pdf cannot be used together")
    }
    // With no supplied title, --doi is the metadata source. Retain the established
    // `--no-pdf --title ... --doi ...` form for manually entered metadata.
    if a.title.is_none() && a.doi.is_some() {
        let value = a.doi.take().unwrap_or_default();
        return add_from_doi(repo, &a, value);
    }
    if !a.no_pdf {
        let p = a
            .pdf
            .as_ref()
            .context("a PDF path is required (or use --no-pdf)")?;
        if !p.is_file() {
            bail!(
                "PDF `{}` does not exist or is not a regular file",
                p.display()
            )
        }
        if !p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")) {
            bail!("file must have a .pdf extension")
        }
    }
    let interactive = io::stdin().is_terminal();
    if a.title.is_none() {
        if !interactive {
            bail!("--title is required when stdin is not interactive")
        }
        a.title = Some(Input::new().with_prompt("Title").interact_text()?);
    }
    if a.author.is_empty() && interactive {
        loop {
            let given = Input::new()
                .with_prompt("Author given name")
                .interact_text()?;
            let family = Input::new()
                .with_prompt("Author family name")
                .interact_text()?;
            a.author.push(Person { given, family });
            if !Confirm::new()
                .with_prompt("Add another author?")
                .default(false)
                .interact()?
            {
                break;
            }
        }
    }
    if a.author.is_empty() {
        eprintln!("warning: reference has no authors")
    }
    a.year = resolve_year(a.year, interactive, || {
        Input::new()
            .with_prompt("Year (leave blank if unknown)")
            .allow_empty(true)
            .interact_text()
            .map_err(Into::into)
    })?;
    let supplied_key = a.key.take();
    let metadata = Reference {
        entry_type: a.entry_type,
        title: a.title.unwrap_or_default(),
        authors: a.author,
        year: a.year,
        container_title: a.container_title,
        publisher: a.publisher,
        volume: None,
        issue: None,
        pages: None,
        doi: a.doi.map(|value| {
            value
                .parse::<Doi>()
                .map_or(value.clone(), |doi| doi.to_string())
        }),
        url: a.url,
        tags: a.tags,
        notes: None,
    };
    let key = add_reference(repo, metadata, supplied_key, a.pdf.as_deref(), interactive)?;
    println!("Added {key}");
    Ok(())
}

fn add_from_doi(repo: &Repository, args: &AddArgs, value: String) -> Result<()> {
    let doi: Doi = value.parse()?;
    if let Some(existing) = find_doi(repo, &doi)? {
        bail!("DOI already exists\n\n{doi} is already stored as `{existing}`");
    }
    eprintln!("Retrieving metadata for DOI {doi}...");
    let client = match env::var("REF_DOI_RESOLVER") {
        Ok(resolver) => {
            DoiMetadataClient::with_resolver(&resolver, std::time::Duration::from_secs(15))?
        }
        Err(_) => DoiMetadataClient::new()?,
    };
    let mut metadata = client.lookup(&doi)?;
    metadata.tags = args.tags.clone();
    let key = add_reference(repo, metadata, args.key.clone(), None, false)?;
    println!("Added {key}");
    Ok(())
}

fn find_doi(repo: &Repository, sought: &Doi) -> Result<Option<CitationKey>> {
    for stored in repo.load_all()? {
        if stored
            .metadata
            .doi
            .as_deref()
            .and_then(|value| value.parse::<Doi>().ok())
            .is_some_and(|doi| &doi == sought)
        {
            return Ok(Some(stored.key));
        }
    }
    Ok(None)
}

fn add_reference(
    repo: &Repository,
    metadata: Reference,
    supplied_key: Option<String>,
    pdf: Option<&Path>,
    interactive: bool,
) -> Result<CitationKey> {
    metadata.validate()?;
    let key = match supplied_key {
        Some(key) => key,
        None => {
            let base = generated_key(&metadata)?.to_string();
            let mut proposed = base.clone();
            let mut collision = 0;
            while repo.contains(&CitationKey::new(&proposed)?) {
                collision += 1;
                proposed = format!("{base}{}", alphabetical_suffix(collision));
            }
            if interactive {
                Input::new()
                    .with_prompt("Citation key")
                    .default(proposed)
                    .interact_text()?
            } else {
                proposed
            }
        }
    };
    let key = CitationKey::new(key)?;
    repo.add(&key, &metadata, pdf)?;
    Ok(key)
}

fn alphabetical_suffix(mut number: usize) -> String {
    let mut suffix = String::new();
    while number > 0 {
        number -= 1;
        suffix.insert(0, (b'A' + (number % 26) as u8) as char);
        number /= 26;
    }
    suffix
}

fn table(items: &[StoredReference]) {
    println!("{:<20} {:<6} {:<22} TITLE", "KEY", "YEAR", "AUTHOR");
    for r in items {
        println!(
            "{:<20} {:<6} {:<22} {}",
            r.key,
            r.metadata.year.map_or("-".into(), |y| y.to_string()),
            display_author(&r.metadata.authors),
            r.metadata.title
        )
    }
}
fn list(repo: &Repository, sort: Sort) -> Result<()> {
    let mut rs = repo.load_all()?;
    rs.sort_by(|a, b| match sort {
        Sort::Key => a.key.cmp(&b.key),
        Sort::Year => a
            .metadata
            .year
            .cmp(&b.metadata.year)
            .then(a.key.cmp(&b.key)),
        Sort::Author => display_author(&a.metadata.authors)
            .to_lowercase()
            .cmp(&display_author(&b.metadata.authors).to_lowercase())
            .then(a.key.cmp(&b.key)),
        Sort::Title => a
            .metadata
            .title
            .to_lowercase()
            .cmp(&b.metadata.title.to_lowercase())
            .then(a.key.cmp(&b.key)),
    });
    table(&rs);
    Ok(())
}
fn rank(r: &StoredReference, q: &str, a: &SearchArgs) -> Option<u8> {
    let key = r.key.as_str().to_lowercase();
    let authors = r
        .metadata
        .authors
        .iter()
        .map(|x| format!("{} {}", x.given, x.family))
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let title = r.metadata.title.to_lowercase();
    let tags = r.metadata.tags.join(" ").to_lowercase();
    let year = r.metadata.year.map(|x| x.to_string()).unwrap_or_default();
    if a.key {
        return key.contains(q).then_some(if key == q {
            0
        } else if key.starts_with(q) {
            1
        } else {
            2
        });
    }
    if a.author {
        return authors.contains(q).then_some(3);
    }
    if a.title {
        return title.contains(q).then_some(4);
    }
    if a.tag {
        return tags.contains(q).then_some(5);
    }
    if a.year {
        return year.contains(q).then_some(6);
    }
    if key == q {
        Some(0)
    } else if key.starts_with(q) {
        Some(1)
    } else if key.contains(q) {
        Some(2)
    } else if authors.contains(q) {
        Some(3)
    } else if title.contains(q) {
        Some(4)
    } else if tags.contains(q) {
        Some(5)
    } else if year.contains(q) {
        Some(6)
    } else {
        None
    }
}
fn search(repo: &Repository, a: SearchArgs) -> Result<()> {
    if [a.author, a.title, a.year, a.tag, a.key]
        .into_iter()
        .filter(|x| *x)
        .count()
        > 1
    {
        bail!("only one search field filter may be used")
    };
    let q = a.query.to_lowercase();
    let mut found: Vec<_> = repo
        .load_all()?
        .into_iter()
        .filter_map(|r| rank(&r, &q, &a).map(|n| (n, r)))
        .collect();
    found.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.key.cmp(&b.1.key)));
    table(&found.into_iter().map(|x| x.1).collect::<Vec<_>>());
    Ok(())
}

fn show(r: &StoredReference) {
    let m = &r.metadata;
    println!(
        "Key:       {}\nType:      {}\nTitle:     {}",
        r.key, m.entry_type, m.title
    );
    println!(
        "Authors:   {}",
        m.authors
            .iter()
            .map(|a| format!("{} {}", a.given, a.family))
            .collect::<Vec<_>>()
            .join(", ")
    );
    if let Some(v) = m.year {
        println!("Year:      {v}")
    }
    for (n, v) in [
        ("Container", &m.container_title),
        ("Publisher", &m.publisher),
        ("Volume", &m.volume),
        ("Issue", &m.issue),
        ("Pages", &m.pages),
        ("DOI", &m.doi),
        ("URL", &m.url),
    ] {
        if let Some(v) = v {
            println!("{n}: {v}")
        }
    }
    if !m.tags.is_empty() {
        println!("Tags:      {}", m.tags.join(", "))
    }
    println!("PDF:       {}", if r.has_pdf { "yes" } else { "no" });
    if let Some(n) = &m.notes {
        println!("\nNotes:\n{n}")
    }
}
fn open_pdf(repo: &Repository, key: String) -> Result<()> {
    launch::open_reference(repo, &CitationKey::new(key)?, &SystemOpener)
}
fn edit(repo: &Repository, key: String) -> Result<()> {
    let key = CitationKey::new(key)?;
    launch::edit_reference(repo, &key, &SystemEnvironment, &SystemEditor)?;
    println!("Updated {key}");
    Ok(())
}
fn remove(repo: &Repository, key: String, yes: bool) -> Result<()> {
    let key = CitationKey::new(key)?;
    let r = repo.load_reference(&key)?;
    let confirmed = if yes {
        true
    } else if io::stdin().is_terminal() {
        println!(
            "{}\n{}\n{}\n{}\n",
            r.key,
            r.metadata
                .authors
                .iter()
                .map(|a| format!("{} {}", a.given, a.family))
                .collect::<Vec<_>>()
                .join(", "),
            r.metadata.title,
            r.metadata.year.map_or("-".into(), |x| x.to_string())
        );
        Confirm::new()
            .with_prompt("Remove this reference and its PDF?")
            .default(false)
            .interact()?
    } else {
        bail!("confirmation required; use --yes in non-interactive mode")
    };
    if confirmed {
        repo.remove(&key)?;
        println!("Removed {key}")
    }
    Ok(())
}

fn clean_cmd(repo: &Repository, args: CleanArgs) -> Result<u8> {
    // Avoid an unnecessary project traversal for an empty collection.
    if repo.load_all()?.is_empty() {
        println!("Nothing to clean.");
        return Ok(0);
    }
    let cwd = env::current_dir()?.canonicalize()?;
    let analysis = clean::analyze(repo, &cwd, &args.paths)?;
    print_clean_analysis(&analysis, args.dry_run);

    if !analysis.scan_errors.is_empty() {
        eprintln!("\nerror: clean scan incomplete\n\nCould not inspect:");
        for error in &analysis.scan_errors {
            eprintln!("  {}: {}", error.path.display(), error.message);
        }
        eprintln!("\nNo references were removed.");
        return Ok(1);
    }
    if analysis.unused.is_empty() {
        println!("\nNothing to clean.");
        return Ok(0);
    }
    if args.dry_run {
        println!("\nDry run: no references were removed.");
        return Ok(0);
    }
    if analysis.files_scanned == 0 {
        eprintln!("warning: no eligible text files were found in the clean scope\n\nAll references would appear unused.");
    }
    let project_root = repo.root().parent().unwrap_or(repo.root());
    if analysis.scan_root != project_root {
        eprintln!("warning: clean is scoped to the current directory\n\nRepository:\n  {}\n\nScan scope:\n  {}\n\nReferences used outside this directory are not considered.\n", project_root.display(), analysis.scan_root.display());
    }
    if !args.yes {
        println!("\n{} unused references found.\n\nThis will remove the reference metadata and any attached PDFs.", analysis.unused.len());
        print!("\nRemove these references? [y/N] ");
        io::stdout().flush()?;
        let mut response = String::new();
        io::stdin().read_line(&mut response)?;
        if !matches!(response.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
            println!("Cleanup cancelled. No references were removed.");
            return Ok(0);
        }
    }
    let mut removed = 0;
    let mut failures = Vec::new();
    for reference in &analysis.unused {
        match repo.remove(&reference.key) {
            Ok(()) => removed += 1,
            Err(error) => failures.push((&reference.key, error)),
        }
    }
    println!("Removed {removed} unused references.");
    if failures.is_empty() {
        return Ok(0);
    }
    eprintln!("\nFailed to remove:");
    for (key, error) in failures {
        eprintln!("  {key}: {error:#}");
    }
    Ok(1)
}

fn print_clean_analysis(analysis: &CleanAnalysis, dry_run: bool) {
    println!(
        "Repository:\n  {}\n\nScan root:\n  {}",
        analysis.repository_root.display(),
        analysis.scan_root.display()
    );
    if analysis.scan_root
        != analysis
            .repository_root
            .parent()
            .unwrap_or(&analysis.repository_root)
    {
        println!("\nNote: files outside this directory were not considered.");
    }
    println!(
        "\nFiles scanned: {}\nReferences:    {}\nUsed:          {}\nUnused:        {}",
        analysis.files_scanned,
        analysis.used.len() + analysis.unused.len(),
        analysis.used.len(),
        analysis.unused.len()
    );
    if analysis.files_scanned == 0 {
        println!("\nwarning: no eligible text files were found in the clean scope\nAll references would appear unused.");
    }
    if !analysis.unused.is_empty() {
        println!(
            "\n{}:",
            if analysis.scan_errors.is_empty() {
                if dry_run {
                    "Would remove"
                } else {
                    "Unused references"
                }
            } else {
                "Tentative unused references"
            }
        );
        for reference in &analysis.unused {
            println!("\n  {}\n    {}", reference.key, reference.metadata.title);
        }
    }
}
fn export_cmd(repo: &Repository, format: &str, output: Option<&Path>) -> Result<()> {
    if format != "biblatex" {
        bail!("unsupported export format `{format}`")
    };
    let data = export::biblatex(&repo.load_all()?);
    if let Some(path) = output {
        let parent = path.parent().unwrap_or(Path::new("."));
        let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
        tmp.write_all(data.as_bytes())?;
        tmp.persist(path).map_err(|e| e.error)?;
    } else {
        print!("{data}")
    }
    Ok(())
}

fn doctor(repo: &Repository, strict: bool) -> Result<u8> {
    let report = doctor::inspect(repo)?;
    let warnings = report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity() == DiagnosticSeverity::Warning)
        .collect::<Vec<_>>();
    let errors = report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity() == DiagnosticSeverity::Error)
        .collect::<Vec<_>>();
    println!("Repository: {}\n\n✓ configuration valid\n✓ {} references discovered\n✓ {} metadata files parsed\n{} {} / {} references have source PDFs", repo.root().display(), report.references_total, report.metadata_parsed, if report.references_without_source_pdf == 0 && errors.iter().all(|d| !matches!(d, r#ref::doctor::DoctorDiagnostic::InvalidSourcePdf { .. })) { "✓" } else { "!" }, report.references_with_source_pdf, report.references_total);
    if !warnings.is_empty() {
        println!("\nWarnings:");
        for w in &warnings {
            println!("  {}", w.message())
        }
    }
    if !errors.is_empty() {
        println!("\nErrors:");
        for e in &errors {
            println!("  {}", e.message())
        }
    }
    println!(
        "\n{} references, {} warnings, {} errors",
        report.references_total,
        report.warning_count(),
        report.error_count()
    );
    Ok(u8::from(
        !errors.is_empty() || (strict && !warnings.is_empty()),
    ))
}

#[cfg(test)]
mod add_tests {
    use super::*;

    #[test]
    fn interactive_year_is_collected_and_invalid_input_is_retried() {
        let mut answers = ["not-a-year", "2024"].into_iter();
        let year = resolve_year(None, true, || Ok(answers.next().unwrap().to_owned())).unwrap();
        let reference = Reference {
            entry_type: ReferenceType::Article,
            title: "Example".into(),
            authors: vec![],
            year,
            container_title: None,
            publisher: None,
            volume: None,
            issue: None,
            pages: None,
            doi: None,
            url: None,
            tags: vec![],
            notes: None,
        };
        assert_eq!(reference.year, Some(2024));
    }

    #[test]
    fn supplied_year_never_prompts() {
        let year = resolve_year(Some(1971), true, || bail!("unexpected prompt")).unwrap();
        assert_eq!(year, Some(1971));
    }
}
