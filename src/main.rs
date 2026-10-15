use std::{collections::BTreeMap, fs, path::PathBuf};

use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand, ValueEnum};
use model_metadata::{
    assertion::{AssertionSet, Review},
    model::{Catalog, RecordKind},
    registry, store, sync,
};
use serde::Serialize;
use serde_json::json;

#[derive(Parser)]
#[command(version, about = "Model metadata with field-level source evidence")]
struct Cli {
    #[arg(long, default_value = ".", global = true)]
    root: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List public sources and sources that require an explicit credential opt-in.
    Sources,
    /// Fetch selected sources; defaults to all public sources.
    Sync {
        #[arg(long, value_delimiter = ',')]
        source: Vec<String>,
        #[arg(long)]
        authenticated: bool,
    },
    /// Validate YAML and references; optionally replay cached raw evidence.
    Validate {
        #[arg(long)]
        evidence: bool,
    },
    /// Export validated catalogs and assertions to deterministic JSON.
    Export {
        #[arg(long, default_value = "dist/registry.json")]
        output: PathBuf,
    },
    /// Count source records, tasks and curated assertions.
    Stats,
    /// Find records by ID or name; output includes each field's evidence pointer.
    Query {
        #[arg(default_value = "")]
        text: String,
        #[arg(long)]
        source: Option<String>,
        #[arg(long, value_enum)]
        kind: Option<Kind>,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Generate JSON Schemas or check committed schemas for drift.
    Schema {
        #[arg(long)]
        check: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Kind {
    Model,
    Service,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Sources => print_json(
            &json!({"public": sync::PUBLIC_SOURCES, "authenticated": sync::AUTHENTICATED_SOURCES}),
        )?,
        Command::Sync {
            source,
            authenticated,
        } => sync::run(&cli.root, &source, authenticated).await?,
        Command::Schema { check } => {
            let settings = schemars::generate::SchemaSettings::draft2020_12();
            for (name, schema) in [
                (
                    "catalog",
                    settings
                        .clone()
                        .into_generator()
                        .into_root_schema_for::<Catalog>(),
                ),
                (
                    "assertions",
                    settings
                        .into_generator()
                        .into_root_schema_for::<AssertionSet>(),
                ),
            ] {
                let path = cli.root.join("schemas").join(format!("{name}.schema.json"));
                let bytes = json_bytes(&schema)?;
                if check {
                    ensure!(
                        fs::read(&path).with_context(|| format!("read {}", path.display()))?
                            == bytes,
                        "schema drift: {}",
                        path.display()
                    );
                } else {
                    store::write_bytes(&path, &bytes)?;
                }
            }
        }
        command => {
            let registry = registry::load(&cli.root)?;
            ensure!(
                !registry.catalogs.is_empty() || !registry.assertions.is_empty(),
                "no data found under {}",
                cli.root.display()
            );
            let cache = cli.root.join(".cache");
            registry.validate(
                matches!(command, Command::Validate { evidence: true }).then_some(cache.as_path()),
            )?;
            match command {
                Command::Validate { evidence } => println!(
                    "Validated {} records and {} assertions{}",
                    registry.records().count(),
                    registry
                        .assertions
                        .iter()
                        .map(|set| set.assertions.len())
                        .sum::<usize>(),
                    if evidence {
                        " with cached evidence"
                    } else {
                        ""
                    }
                ),
                Command::Export { output } => {
                    let path = cli.root.join(output);
                    store::write_bytes(&path, &json_bytes(&registry.export())?)?;
                    println!("{}", path.display());
                }
                Command::Stats => {
                    let mut sources = BTreeMap::new();
                    let mut tasks = BTreeMap::<String, usize>::new();
                    for catalog in &registry.catalogs {
                        sources.insert(&catalog.source, json!({"models": catalog.records.iter().filter(|r| r.kind == RecordKind::Model).count(), "services": catalog.records.iter().filter(|r| r.kind == RecordKind::Service).count(), "snapshots": catalog.snapshots.len()}));
                    }
                    for record in registry.records() {
                        for key in ["pipeline_tag", "task", "type"] {
                            if let Some(task) =
                                record.facts.get(key).and_then(|fact| fact.value.as_str())
                            {
                                *tasks.entry(task.to_owned()).or_default() += 1;
                            }
                        }
                    }
                    print_json(
                        &json!({"sources": sources, "task_facts": tasks, "assertions": registry.assertions.iter().map(|set| set.assertions.len()).sum::<usize>(), "conflicts": registry.assertions.iter().flat_map(|set| &set.assertions).filter(|a| a.review == Review::Conflicted).count()}),
                    )?;
                }
                Command::Query {
                    text,
                    source,
                    kind,
                    limit,
                } => {
                    let text = text.to_lowercase();
                    let kind = kind.map(|kind| match kind {
                        Kind::Model => RecordKind::Model,
                        Kind::Service => RecordKind::Service,
                    });
                    let mut records: Vec<_> = registry
                        .records()
                        .filter(|r| {
                            source.as_ref().is_none_or(|s| r.source == *s)
                                && kind.is_none_or(|kind| r.kind == kind)
                                && (r.id.to_lowercase().contains(&text)
                                    || r.name.to_lowercase().contains(&text))
                        })
                        .collect();
                    records.sort_by(|a, b| a.id.cmp(&b.id));
                    records.truncate(limit);
                    print_json(&records)?;
                }
                _ => unreachable!(),
            }
        }
    }
    Ok(())
}

fn json_bytes(value: &impl Serialize) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn print_json(value: &impl Serialize) -> Result<()> {
    use std::io::Write;
    std::io::stdout().lock().write_all(&json_bytes(value)?)?;
    Ok(())
}
