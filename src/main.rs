use anyhow::{bail, Context, Result};
mod output;
use clap::{Args, Parser, Subcommand, ValueEnum};
use dialoguer::{Confirm, Input};
use output::{
    BatchDiagnostic, CommandOutput, DiagnosticOutput, OperationStatus, OutputFormat,
    ReferenceOutput, WarningOutput,
};
use r#ref::{
    clean,
    doctor::{self, DiagnosticSeverity},
    export, import,
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
    /// Emit machine-readable JSON instead of human-readable output
    #[arg(long, global = true)]
    json: bool,
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

struct Execution {
    command: &'static str,
    output: CommandOutput,
    status: OperationStatus,
    warnings: Vec<WarningOutput>,
    exit_code: u8,
}
impl Execution {
    fn success(command: &'static str, output: CommandOutput) -> Self {
        Self {
            command,
            output,
            status: OperationStatus::Success,
            warnings: vec![],
            exit_code: 0,
        }
    }
}

fn main() -> ExitCode {
    let arguments = env::args_os().collect::<Vec<_>>();
    let requested_json = arguments.iter().any(|argument| argument == "--json");
    let parsed_command = arguments
        .iter()
        .skip(1)
        .filter_map(|argument| argument.to_str())
        .find(|argument| !argument.starts_with('-'))
        .unwrap_or("ref")
        .to_owned();
    let cli = match Cli::try_parse_from(arguments) {
        Ok(cli) => cli,
        Err(error) if requested_json => {
            output::render_error(
                OutputFormat::Json,
                &parsed_command,
                &anyhow::anyhow!(error.to_string()),
            );
            return ExitCode::from(2);
        }
        Err(error) => error.exit(),
    };
    let format = if cli.json {
        OutputFormat::Json
    } else {
        OutputFormat::Human
    };
    let command = command_name(&cli.command);
    match execute(cli.command, format) {
        Ok(done) => match output::render(
            format,
            done.command,
            done.status,
            &done.warnings,
            &done.output,
        ) {
            Ok(()) => ExitCode::from(done.exit_code),
            Err(error) => {
                output::render_error(format, command, &error);
                ExitCode::from(1)
            }
        },
        Err(error) => {
            output::render_error(format, command, &error);
            ExitCode::from(1)
        }
    }
}
fn command_name(c: &Commands) -> &'static str {
    match c {
        Commands::Init => "init",
        Commands::Add(_) => "add",
        Commands::List { .. } => "list",
        Commands::Search(_) => "search",
        Commands::Show { .. } => "show",
        Commands::Open { .. } => "open",
        Commands::Attach { .. } => "attach",
        Commands::Edit { .. } => "edit",
        Commands::Rename { .. } => "rename",
        Commands::Remove { .. } => "remove",
        Commands::Clean(_) => "clean",
        Commands::Export { .. } => "export",
        Commands::Import { .. } => "import",
        Commands::Doctor { .. } => "doctor",
    }
}

fn execute(command: Commands, format: OutputFormat) -> Result<Execution> {
    Ok(match command {
        Commands::Init => {
            let cwd = env::current_dir()?.canonicalize()?;
            let r = Repository::init(&cwd)?;
            Execution::success(
                "init",
                CommandOutput::Init {
                    repository_root_path: cwd,
                    reference_repository_path: r.root().to_path_buf(),
                    repository_format_version: 1,
                },
            )
        }
        Commands::Add(a) => add(&repo()?, a, format)?,
        Commands::List { sort } => list(&repo()?, sort)?,
        Commands::Search(a) => search(&repo()?, a)?,
        Commands::Show { key } => {
            let r = repo()?.load_reference(&CitationKey::new(key)?)?;
            Execution::success(
                "show",
                CommandOutput::Show(ReferenceOutput::from_stored(&r)),
            )
        }
        Commands::Open { key } => {
            let repo = repo()?;
            let key = CitationKey::new(key)?;
            let r = repo.load_reference(&key)?;
            let path = r.path.join("paper.pdf");
            launch::open_reference(&repo, &key, &SystemOpener)?;
            Execution::success(
                "open",
                CommandOutput::Open {
                    citation_key: key.to_string(),
                    source_pdf_path: path,
                    external_viewer_launch_requested: true,
                },
            )
        }
        Commands::Attach { key, pdf } => {
            let repo = repo()?;
            let key = CitationKey::new(key)?;
            repo.attach(&key, &pdf)?;
            Execution::success(
                "attach",
                CommandOutput::Attach {
                    citation_key: key.to_string(),
                    source_pdf_path: repo.reference_path(&key).join("paper.pdf"),
                },
            )
        }
        Commands::Edit { key } => {
            if format == OutputFormat::Json {
                bail!("edit is unavailable in non-interactive JSON mode")
            }
            let repo = repo()?;
            let key = CitationKey::new(key)?;
            launch::edit_reference(&repo, &key, &SystemEnvironment, &SystemEditor)?;
            Execution::success(
                "edit",
                CommandOutput::Edit {
                    citation_key: key.to_string(),
                    metadata_file_path: repo.reference_path(&key).join("ref.yaml"),
                    metadata_valid_after_edit: true,
                },
            )
        }
        Commands::Rename { old_key, new_key } => {
            let repo = repo()?;
            let old = CitationKey::new(old_key)?;
            let new = CitationKey::new(new_key)?;
            repo.rename(&old, &new)?;
            let mut e = Execution::success(
                "rename",
                CommandOutput::Rename {
                    previous_citation_key: old.to_string(),
                    new_citation_key: new.to_string(),
                    reference_directory_path: repo.reference_path(&new),
                },
            );
            e.warnings.push(WarningOutput {
                warning_code: "citations_not_rewritten".into(),
                message: "Existing project citations were not rewritten.".into(),
                citation_key: Some(old.to_string()),
            });
            e
        }
        Commands::Remove { key, yes } => remove(&repo()?, key, yes, format)?,
        Commands::Clean(a) => clean_cmd(&repo()?, a, format)?,
        Commands::Export { format: f, output } => export_cmd(&repo()?, &f, output.as_deref())?,
        Commands::Import { file } => import_cmd(&repo()?, &file)?,
        Commands::Doctor { strict } => doctor_cmd(&repo()?, strict)?,
    })
}

fn reference_output(repo: &Repository, key: &CitationKey) -> Result<ReferenceOutput> {
    Ok(ReferenceOutput::from_stored(&repo.load_reference(key)?))
}
fn add(repo: &Repository, mut a: AddArgs, format: OutputFormat) -> Result<Execution> {
    if a.no_pdf && a.pdf.is_some() {
        bail!("a PDF and --no-pdf cannot be used together")
    }
    if a.title.is_none() && a.doi.is_some() {
        return add_from_doi(repo, &a, a.doi.clone().unwrap_or_default(), format);
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
    let interactive = format == OutputFormat::Human && io::stdin().is_terminal();
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
    a.year = resolve_year(a.year, interactive, || {
        Input::new()
            .with_prompt("Year (leave blank if unknown)")
            .allow_empty(true)
            .interact_text()
            .map_err(Into::into)
    })?;
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
        doi: a
            .doi
            .map(|v| v.parse::<Doi>().map_or(v.clone(), |d| d.to_string())),
        url: a.url,
        tags: a.tags,
        notes: None,
    };
    let key = add_reference(repo, metadata, a.key.take(), a.pdf.as_deref(), interactive)?;
    let out = reference_output(repo, &key)?;
    let path = repo.reference_path(&key);
    let mut e = Execution::success(
        "add",
        CommandOutput::Add {
            created_reference: out,
            reference_directory_path: path,
        },
    );
    if repo.load_reference(&key)?.metadata.authors.is_empty() {
        e.warnings.push(WarningOutput {
            warning_code: "missing_authors".into(),
            message: "Reference has no authors.".into(),
            citation_key: Some(key.to_string()),
        });
    }
    Ok(e)
}
fn add_from_doi(
    repo: &Repository,
    args: &AddArgs,
    value: String,
    format: OutputFormat,
) -> Result<Execution> {
    let doi: Doi = value.parse()?;
    if let Some(existing) = find_doi(repo, &doi)? {
        bail!("DOI already exists\n\n{doi} is already stored as `{existing}`")
    }
    if format == OutputFormat::Human {
        eprintln!("Retrieving metadata for DOI {doi}...");
    }
    let client = match env::var("REF_DOI_RESOLVER") {
        Ok(r) => DoiMetadataClient::with_resolver(&r, std::time::Duration::from_secs(15))?,
        Err(_) => DoiMetadataClient::new()?,
    };
    let mut m = client.lookup(&doi)?;
    m.tags = args.tags.clone();
    let key = add_reference(repo, m, args.key.clone(), None, false)?;
    Ok(Execution::success(
        "add",
        CommandOutput::Add {
            created_reference: reference_output(repo, &key)?,
            reference_directory_path: repo.reference_path(&key),
        },
    ))
}
fn find_doi(repo: &Repository, sought: &Doi) -> Result<Option<CitationKey>> {
    for r in repo.load_all()? {
        if r.metadata
            .doi
            .as_deref()
            .and_then(|v| v.parse::<Doi>().ok())
            .is_some_and(|d| &d == sought)
        {
            return Ok(Some(r.key));
        }
    }
    Ok(None)
}
fn add_reference(
    repo: &Repository,
    metadata: Reference,
    supplied: Option<String>,
    pdf: Option<&Path>,
    interactive: bool,
) -> Result<CitationKey> {
    metadata.validate()?;
    let key = match supplied {
        Some(k) => k,
        None => {
            let base = generated_key(&metadata)?.to_string();
            let mut proposed = base.clone();
            let mut n = 0;
            while repo.contains(&CitationKey::new(&proposed)?) {
                n += 1;
                proposed = format!("{base}{}", alphabetical_suffix(n));
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
fn alphabetical_suffix(mut n: usize) -> String {
    let mut s = String::new();
    while n > 0 {
        n -= 1;
        s.insert(0, (b'A' + (n % 26) as u8) as char);
        n /= 26;
    }
    s
}
fn list(repo: &Repository, sort: Sort) -> Result<Execution> {
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
    let references = rs
        .iter()
        .map(ReferenceOutput::from_stored)
        .collect::<Vec<_>>();
    Ok(Execution::success(
        "list",
        CommandOutput::List {
            reference_count: references.len(),
            references,
        },
    ))
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
fn search(repo: &Repository, a: SearchArgs) -> Result<Execution> {
    if [a.author, a.title, a.year, a.tag, a.key]
        .into_iter()
        .filter(|x| *x)
        .count()
        > 1
    {
        bail!("only one search field filter may be used")
    };
    let q = a.query.to_lowercase();
    let mut found = repo
        .load_all()?
        .into_iter()
        .filter_map(|r| rank(&r, &q, &a).map(|n| (n, r)))
        .collect::<Vec<_>>();
    found.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.key.cmp(&b.1.key)));
    let refs = found
        .iter()
        .map(|x| ReferenceOutput::from_stored(&x.1))
        .collect::<Vec<_>>();
    let field = if a.author {
        "author"
    } else if a.title {
        "title"
    } else if a.year {
        "year"
    } else if a.tag {
        "tag"
    } else if a.key {
        "citation_key"
    } else {
        "all"
    };
    Ok(Execution::success(
        "search",
        CommandOutput::Search {
            search_query: a.query,
            search_field: field.into(),
            matching_reference_count: refs.len(),
            matching_references: refs,
        },
    ))
}
fn remove(repo: &Repository, key: String, yes: bool, format: OutputFormat) -> Result<Execution> {
    let key = CitationKey::new(key)?;
    let r = repo.load_reference(&key)?;
    if !yes {
        if format == OutputFormat::Json || !io::stdin().is_terminal() {
            bail!("confirmation required; use --yes in non-interactive mode")
        }
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
        if !Confirm::new()
            .with_prompt("Remove this reference and its PDF?")
            .default(false)
            .interact()?
        {
            return Ok(Execution::success(
                "remove",
                CommandOutput::Remove {
                    removed_citation_key: key.to_string(),
                    reference_removed: false,
                    source_pdf_removed: false,
                },
            ));
        }
    }
    let had = r.has_pdf;
    repo.remove(&key)?;
    Ok(Execution::success(
        "remove",
        CommandOutput::Remove {
            removed_citation_key: key.to_string(),
            reference_removed: true,
            source_pdf_removed: had,
        },
    ))
}
fn clean_cmd(repo: &Repository, args: CleanArgs, format: OutputFormat) -> Result<Execution> {
    let cwd = env::current_dir()?.canonicalize()?;
    let analysis = clean::analyze(repo, &cwd, &args.paths)?;
    let total = analysis.used.len() + analysis.unused.len();
    let unused = analysis
        .unused
        .iter()
        .map(ReferenceOutput::from_stored)
        .collect::<Vec<_>>();
    if !analysis.scan_errors.is_empty() {
        bail!(
            "clean scan incomplete: {}",
            analysis
                .scan_errors
                .iter()
                .map(|e| format!("{}: {}", e.path.display(), e.message))
                .collect::<Vec<_>>()
                .join("; ")
        )
    }
    if !args.dry_run && !analysis.unused.is_empty() && !args.yes {
        if format == OutputFormat::Json || !io::stdin().is_terminal() {
            bail!("confirmation required; use --yes in non-interactive mode")
        }
        print!("\nRemove these references? [y/N] ");
        io::stdout().flush()?;
        let mut response = String::new();
        io::stdin().read_line(&mut response)?;
        if !matches!(response.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
            return Ok(Execution::success(
                "clean",
                CommandOutput::Clean {
                    dry_run: false,
                    repository_root_path: analysis.repository_root,
                    scan_root_path: analysis.scan_root,
                    eligible_files_scanned: analysis.files_scanned,
                    reference_count: total,
                    used_reference_count: analysis.used.len(),
                    unused_reference_count: analysis.unused.len(),
                    unused_references: unused,
                    references_removed: vec![],
                    failed_references: vec![],
                },
            ));
        }
    }
    let mut removed = vec![];
    let mut failed = vec![];
    if !args.dry_run {
        for r in &analysis.unused {
            match repo.remove(&r.key) {
                Ok(()) => removed.push(r.key.to_string()),
                Err(e) => failed.push(BatchDiagnostic {
                    citation_key: r.key.to_string(),
                    diagnostic_code: "remove_failed".into(),
                    message: format!("{e:#}"),
                }),
            }
        }
    }
    let partial = !failed.is_empty();
    Ok(Execution {
        command: "clean",
        output: CommandOutput::Clean {
            dry_run: args.dry_run,
            repository_root_path: analysis.repository_root,
            scan_root_path: analysis.scan_root,
            eligible_files_scanned: analysis.files_scanned,
            reference_count: total,
            used_reference_count: analysis.used.len(),
            unused_reference_count: analysis.unused.len(),
            unused_references: unused,
            references_removed: removed,
            failed_references: failed,
        },
        status: if partial {
            OperationStatus::PartialSuccess
        } else {
            OperationStatus::Success
        },
        warnings: vec![],
        exit_code: u8::from(partial),
    })
}
fn export_cmd(repo: &Repository, format: &str, path: Option<&Path>) -> Result<Execution> {
    if format != "biblatex" {
        bail!("unsupported export format `{format}`")
    }
    let refs = repo.load_all()?;
    let data = export::biblatex(&refs);
    let (content, out, written) = if let Some(path) = path {
        let absolute = absolute_path(path)?;
        let parent = absolute.parent().unwrap_or(Path::new("."));
        let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
        tmp.write_all(data.as_bytes())?;
        tmp.persist(&absolute).map_err(|e| e.error)?;
        (None, Some(absolute), true)
    } else {
        (Some(data), None, false)
    };
    Ok(Execution::success(
        "export",
        CommandOutput::Export {
            export_format: "biblatex".into(),
            reference_count: refs.len(),
            bibliography_content: content,
            output_file_path: out,
            bibliography_written_to_file: written,
        },
    ))
}
fn import_cmd(repo: &Repository, path: &Path) -> Result<Execution> {
    if !path.is_file() {
        bail!(
            "bibliography `{}` does not exist or is not a regular file",
            path.display()
        )
    }
    let absolute = absolute_path(path)?;
    let source =
        fs::read_to_string(path).with_context(|| format!("failed to read `{}`", path.display()))?;
    let entries = import::parse_bibliography(&source)
        .with_context(|| format!("failed to parse `{}`", path.display()))?;
    let keys = entries.iter().map(|e| e.key.clone()).collect::<Vec<_>>();
    let result = import::import_bibliography(repo, entries);
    let imported = keys
        .into_iter()
        .filter(|k| {
            !result
                .skipped
                .iter()
                .chain(result.failed.iter())
                .any(|d| d.key == *k)
        })
        .collect();
    let convert = |d: &import::ImportDiagnostic| BatchDiagnostic {
        citation_key: d.key.clone(),
        diagnostic_code: format!("{:?}", d.kind).to_ascii_lowercase(),
        message: d.message.clone(),
    };
    let skipped = result.skipped.iter().map(convert).collect::<Vec<_>>();
    let failed = result.failed.iter().map(convert).collect::<Vec<_>>();
    let partial = !result.is_complete();
    let warnings = result
        .warnings
        .iter()
        .map(|d| WarningOutput {
            warning_code: "unsupported_reference_type".into(),
            message: d.message.clone(),
            citation_key: Some(d.key.clone()),
        })
        .collect();
    Ok(Execution {
        command: "import",
        output: CommandOutput::Import {
            input_bibliography_path: absolute,
            entries_found: result.total,
            references_imported: result.imported,
            references_skipped: skipped.len(),
            references_failed: failed.len(),
            imported_citation_keys: imported,
            skipped_references: skipped,
            failed_references: failed,
        },
        status: if partial {
            OperationStatus::PartialSuccess
        } else {
            OperationStatus::Success
        },
        warnings,
        exit_code: u8::from(partial),
    })
}
fn doctor_cmd(repo: &Repository, strict: bool) -> Result<Execution> {
    let report = doctor::inspect(repo)?;
    let diagnostics = report
        .diagnostics
        .iter()
        .map(|d| {
            let severity = match d.severity() {
                DiagnosticSeverity::Warning => "warning",
                DiagnosticSeverity::Error => "error",
            };
            let message = d.message();
            let citation_key = message
                .split(':')
                .next()
                .filter(|s| !s.contains(' '))
                .map(str::to_owned);
            DiagnosticOutput {
                severity: severity.into(),
                diagnostic_code: doctor_code(d).into(),
                citation_key,
                message,
            }
        })
        .collect();
    let failed = report.error_count() > 0 || (strict && report.warning_count() > 0);
    Ok(Execution {
        command: "doctor",
        output: CommandOutput::Doctor {
            repository_root_path: repo.root().to_path_buf(),
            reference_count: report.references_total,
            references_with_source_pdf: report.references_with_source_pdf,
            references_without_source_pdf: report.references_without_source_pdf,
            warning_count: report.warning_count(),
            error_count: report.error_count(),
            strict_mode: strict,
            diagnostics,
        },
        status: if failed {
            OperationStatus::Failure
        } else {
            OperationStatus::Success
        },
        warnings: vec![],
        exit_code: u8::from(failed),
    })
}
fn doctor_code(d: &r#ref::doctor::DoctorDiagnostic) -> &'static str {
    use r#ref::doctor::DoctorDiagnostic::*;
    match d {
        InvalidCitationKey { .. } => "invalid_citation_key",
        MissingMetadata { .. } => "missing_metadata",
        InvalidMetadata { .. } => "invalid_metadata",
        MissingYear { .. } => "missing_publication_year",
        MissingAuthors { .. } => "missing_authors",
        MalformedDoi { .. } => "malformed_doi",
        DuplicateDoi { .. } => "duplicate_doi",
        MissingSourcePdf { .. } => "missing_source_pdf",
        InvalidSourcePdf { .. } => "invalid_source_pdf",
    }
}
fn absolute_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(env::current_dir()?.join(path))
    }
}

#[cfg(test)]
mod add_tests {
    use super::*;
    #[test]
    fn suffixes() {
        assert_eq!(alphabetical_suffix(1), "A");
        assert_eq!(alphabetical_suffix(27), "AA");
    }
    #[test]
    fn supplied_year_never_prompts() {
        let year = resolve_year(Some(1971), true, || bail!("unexpected prompt")).unwrap();
        assert_eq!(year, Some(1971));
    }
}
