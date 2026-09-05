use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use dialoguer::{Confirm, Input};
use r#ref::{
    export,
    model::{display_author, generated_key, CitationKey, Person, Reference, ReferenceType},
    repository::{Repository, StoredReference},
};
use std::{
    collections::HashMap,
    env, fs,
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

#[derive(Parser)]
#[command(
    name = "ref",
    version,
    about = "A Git-like reference manager for scientific writing"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Init,
    Add(AddArgs),
    List {
        #[arg(long, value_enum, default_value = "key")]
        sort: Sort,
    },
    Search(SearchArgs),
    Show {
        key: String,
    },
    Open {
        key: String,
    },
    Edit {
        key: String,
    },
    Rename {
        old_key: String,
        new_key: String,
    },
    #[command(alias = "rm")]
    Remove {
        key: String,
        #[arg(long, short)]
        yes: bool,
    },
    Export {
        #[arg(default_value = "biblatex")]
        format: String,
        #[arg(long, short)]
        output: Option<PathBuf>,
    },
    Doctor {
        #[arg(long)]
        strict: bool,
    },
}
#[derive(Args)]
struct AddArgs {
    pdf: Option<PathBuf>,
    #[arg(long)]
    no_pdf: bool,
    #[arg(long)]
    key: Option<String>,
    #[arg(long)]
    title: Option<String>,
    #[arg(long, value_parser=parse_author)]
    author: Vec<Person>,
    #[arg(long)]
    year: Option<u16>,
    #[arg(long = "type", default_value = "article")]
    entry_type: ReferenceType,
    #[arg(long)]
    container_title: Option<String>,
    #[arg(long)]
    publisher: Option<String>,
    #[arg(long)]
    doi: Option<String>,
    #[arg(long)]
    url: Option<String>,
    #[arg(long, value_delimiter = ',')]
    tags: Vec<String>,
}
#[derive(Args)]
struct SearchArgs {
    query: String,
    #[arg(long)]
    author: bool,
    #[arg(long)]
    title: bool,
    #[arg(long)]
    year: bool,
    #[arg(long)]
    tag: bool,
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
    let (given, family) = s.split_once('|').ok_or("author must use given|family")?;
    if family.trim().is_empty() {
        return Err("author family name cannot be empty".into());
    }
    Ok(Person {
        given: given.into(),
        family: family.into(),
    })
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
        Commands::Edit { key } => edit(&repo()?, key)?,
        Commands::Rename { old_key, new_key } => {
            let old = CitationKey::new(old_key)?;
            let new = CitationKey::new(new_key)?;
            repo()?.rename(&old, &new)?;
            println!("Renamed {old} → {new}\n\nNote: existing \\cite{{{old}}} references are not updated automatically.");
        }
        Commands::Remove { key, yes } => remove(&repo()?, key, yes)?,
        Commands::Export { format, output } => export_cmd(&repo()?, &format, output.as_deref())?,
        Commands::Doctor { strict } => return doctor(&repo()?, strict),
    };
    Ok(0)
}

fn add(repo: &Repository, mut a: AddArgs) -> Result<()> {
    if a.no_pdf && a.pdf.is_some() {
        bail!("a PDF and --no-pdf cannot be used together")
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
    let base = generated_key(a.author.first().map_or("reference", |x| &x.family), a.year);
    let mut proposed = base.clone();
    let mut suffix = b'a';
    while repo.contains(&CitationKey::new(&proposed)?) {
        proposed = format!("{base}{}", suffix as char);
        suffix += 1;
    }
    let key = match a.key {
        Some(k) => k,
        None if interactive => Input::new()
            .with_prompt("Citation key")
            .default(proposed)
            .interact_text()?,
        None => bail!("--key is required when stdin is not interactive"),
    };
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
        doi: a.doi,
        url: a.url,
        tags: a.tags,
        notes: None,
    };
    let key = CitationKey::new(key)?;
    repo.add(&key, &metadata, a.pdf.as_deref())?;
    println!("Added {key}");
    Ok(())
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
    let r = repo.load_reference(&CitationKey::new(key)?)?;
    let pdf = r.path.join("paper.pdf");
    if !pdf.is_file() {
        bail!("reference `{}` has no PDF", r.key)
    }
    open::that_detached(&pdf).with_context(|| format!("failed to open {}", pdf.display()))?;
    Ok(())
}
fn edit(repo: &Repository, key: String) -> Result<()> {
    let r = repo.load_reference(&CitationKey::new(key)?)?;
    let editor = env::var("VISUAL")
        .or_else(|_| env::var("EDITOR"))
        .map_err(|_| {
            anyhow::anyhow!(
                "no editor configured\nhint: set the VISUAL or EDITOR environment variable"
            )
        })?;
    let mut parts = editor.split_whitespace();
    let program = parts.next().context("editor command is empty")?;
    let status = Command::new(program)
        .args(parts)
        .arg(r.path.join("ref.yaml"))
        .status()?;
    if !status.success() {
        bail!("editor exited unsuccessfully")
    };
    repo.load_reference(&r.key)?;
    println!("Updated {}", r.key);
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

fn valid_doi(s: &str) -> bool {
    let Some((prefix, suffix)) = s.split_once('/') else {
        return false;
    };
    prefix.starts_with("10.")
        && prefix[3..].chars().all(|c| c.is_ascii_digit())
        && prefix.len() > 3
        && !suffix.trim().is_empty()
        && !s.chars().any(char::is_whitespace)
}
fn doctor(repo: &Repository, strict: bool) -> Result<u8> {
    repo.validate_structure()?;
    let mut warnings = Vec::new();
    let mut errors = Vec::new();
    let mut parsed = 0;
    let mut pdfs = 0;
    let mut count = 0;
    let mut dois: HashMap<String, Vec<String>> = HashMap::new();
    for entry in fs::read_dir(repo.references_dir())? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        count += 1;
        let name = entry.file_name().to_string_lossy().into_owned();
        if CitationKey::new(&name).is_err() {
            errors.push(format!("{name}: invalid citation key"));
            continue;
        }
        let yaml = entry.path().join("ref.yaml");
        if !yaml.is_file() {
            errors.push(format!("{name}: ref.yaml missing"));
            continue;
        }
        let text = match fs::read_to_string(&yaml) {
            Ok(x) => x,
            Err(e) => {
                errors.push(format!("{name}: cannot read metadata: {e}"));
                continue;
            }
        };
        let metadata: Reference = match serde_yaml::from_str(&text) {
            Ok(x) => x,
            Err(e) => {
                errors.push(format!("{name}: invalid metadata: {e}"));
                continue;
            }
        };
        parsed += 1;
        if let Err(e) = metadata.validate() {
            errors.push(format!("{name}: invalid metadata: {e}"))
        }
        if metadata.year.is_none() {
            warnings.push(format!("{name}: year missing"))
        }
        if metadata.authors.is_empty() {
            warnings.push(format!("{name}: authors missing"))
        }
        if let Some(doi) = metadata.doi {
            if !valid_doi(&doi) {
                errors.push(format!("{name}: malformed DOI"))
            }
            dois.entry(doi.to_lowercase())
                .or_default()
                .push(name.clone());
        }
        let pdf = entry.path().join("paper.pdf");
        if pdf.exists() {
            if pdf.is_file() {
                pdfs += 1
            } else {
                errors.push(format!("{name}: paper.pdf is not a regular file"))
            }
        }
    }
    for keys in dois.values().filter(|v| v.len() > 1) {
        warnings.push(format!("duplicate DOI: {}", keys.join(", ")))
    }
    println!("Repository: {}\n\n✓ configuration valid\n✓ {count} references discovered\n✓ {parsed} metadata files parsed\n✓ {pdfs} PDFs found",repo.root().display());
    if !warnings.is_empty() {
        println!("\nWarnings:");
        for w in &warnings {
            println!("  {w}")
        }
    }
    if !errors.is_empty() {
        println!("\nErrors:");
        for e in &errors {
            println!("  {e}")
        }
    }
    println!(
        "\n{count} references, {} warnings, {} errors",
        warnings.len(),
        errors.len()
    );
    Ok(u8::from(
        !errors.is_empty() || (strict && !warnings.is_empty()),
    ))
}
