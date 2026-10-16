use std::{collections::BTreeMap, fs, path::PathBuf};

use anyhow::{Context, Result, ensure};
use clap::{Args, Parser, Subcommand, ValueEnum};
use model_metadata::{
    assertion::{AssertionSet, Review, Scope, ScopeKind},
    discovery, documents,
    domain::{self, DomainMetadata},
    evidence,
    identity::{self, IdentitySet},
    model::{Catalog, Record, RecordKind},
    registry::{self, Registry},
    relations, rules, store, sync,
};
use serde::Serialize;
use serde_json::{Value, json};

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
    /// List source coverage, authentication and terms metadata.
    Sources,
    /// Fetch selected sources; defaults to all public sources.
    Sync {
        #[arg(long, value_delimiter = ',')]
        source: Vec<String>,
        #[arg(long)]
        authenticated: bool,
    },
    /// Discover bounded public candidates for review before adding seeds.
    Discover {
        #[arg(long, value_delimiter = ',')]
        source: Vec<String>,
        #[arg(long, default_value = "dist/discovery.json")]
        output: PathBuf,
    },
    /// Collect official vendor pages and compare their model claims.
    Documents {
        #[command(subcommand)]
        command: DocumentCommand,
    },
    /// Validate catalogs, typed fields and identities; optionally replay raw evidence.
    Validate {
        #[arg(long)]
        evidence: bool,
    },
    /// Export raw records, typed profiles, identities and relationships.
    Export {
        #[arg(long, default_value = "dist/registry.json")]
        output: PathBuf,
    },
    /// Count source records, typed coverage, relationships and assertions.
    Stats,
    /// Find records by name and explicit metadata; missing facts do not match filters.
    Query(Query),
    /// Inspect one record, its typed fields, explicit bindings and rule candidates.
    Inspect { record: String },
    /// List curated identities, optionally restricted to one record.
    Identities {
        #[arg(long)]
        record: Option<String>,
    },
    /// List explicit source relationships, optionally touching one record.
    Relations {
        #[arg(long)]
        record: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Evaluate documented conditions while preserving unknowns and conflicts.
    Rules {
        #[arg(long)]
        record: Option<String>,
        #[arg(long, requires = "scope_kind")]
        scope: Option<String>,
        #[arg(long, value_enum, requires = "scope")]
        scope_kind: Option<RuleScope>,
        #[arg(long)]
        property: Option<String>,
        /// JSON object with literal condition paths as keys.
        #[arg(long, default_value = "{}")]
        context: String,
    },
    /// Copy and verify exact referenced snapshots without contacting upstream APIs.
    Evidence {
        #[command(subcommand)]
        command: EvidenceCommand,
    },
    /// Generate JSON Schemas or check committed schemas for drift.
    Schema {
        #[arg(long)]
        check: bool,
    },
}

#[derive(Args)]
struct Query {
    #[arg(default_value = "")]
    text: String,
    #[arg(long)]
    source: Option<String>,
    #[arg(long, value_enum)]
    kind: Option<Kind>,
    #[arg(long)]
    task: Option<String>,
    #[arg(long)]
    input: Option<String>,
    #[arg(long)]
    output: Option<String>,
    #[arg(long)]
    min_context: Option<u64>,
    #[arg(long, value_enum)]
    capability: Option<Capability>,
    /// Match curated model members and services explicitly linked to those models.
    #[arg(long)]
    identity: Option<String>,
    #[arg(long, default_value_t = 20)]
    limit: usize,
}

#[derive(Clone, Copy, ValueEnum)]
enum Kind {
    Model,
    Service,
}

#[derive(Clone, Copy, ValueEnum)]
enum RuleScope {
    Model,
    Interface,
}

#[derive(Clone, Copy, ValueEnum)]
enum Capability {
    Reasoning,
    Thinking,
    ToolCall,
    StructuredOutput,
    OpenWeights,
    Attachment,
    TemperatureControl,
}

#[derive(Subcommand)]
enum EvidenceCommand {
    Archive {
        #[arg(long)]
        output: PathBuf,
    },
    Restore {
        #[arg(long)]
        input: PathBuf,
    },
}

#[derive(Subcommand)]
enum DocumentCommand {
    /// Fetch selected official pages; failures are saved and reported.
    Sync {
        #[arg(long)]
        vendor: Option<String>,
    },
    /// Re-extract cached pages without changing fetch dates or contacting vendors.
    Reparse,
    /// Show authoritative field choices and all supporting or conflicting evidence.
    Resolve { record: String },
    /// List source URLs, metadata, claims and fetch failures.
    List {
        #[arg(long)]
        record: Option<String>,
        #[arg(long)]
        vendor: Option<String>,
    },
    /// Compare exact model claims with source catalog facts.
    Check {
        #[arg(long)]
        record: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Sources => print_json(&discovery::load_sources(&cli.root)?)?,
        Command::Sync {
            source,
            authenticated,
        } => sync::run(&cli.root, &source, authenticated).await?,
        Command::Discover { source, output } => {
            let report = discovery::run(&cli.root, &source, &output).await?;
            print_json(&report)?;
            ensure!(
                !report.has_failures(),
                "discovery had failures; completed results are saved in {}",
                cli.root.join(output).display()
            );
        }
        Command::Documents {
            command: DocumentCommand::Sync { vendor },
        } => {
            let catalog = documents::sync(&cli.root, vendor.as_deref()).await?;
            let selected: Vec<_> = catalog
                .documents
                .iter()
                .filter(|d| vendor.as_ref().is_none_or(|v| d.source.vendor == *v))
                .collect();
            let failed: Vec<_> = selected
                .iter()
                .filter(|d| d.error.is_some())
                .map(|d| &d.source.id)
                .collect();
            print_json(&json!({"checked":selected.len(),"failed":failed,"documents":selected}))?;
            ensure!(
                failed.is_empty(),
                "official page failures saved in data/documents/official.yaml"
            );
        }
        Command::Documents {
            command: DocumentCommand::Reparse,
        } => {
            let catalog = documents::reparse(&cli.root)?;
            print_json(
                &json!({"reparsed":catalog.documents.iter().filter(|d|d.content.is_some()).count(),"extractor_version":documents::EXTRACTOR_VERSION}),
            )?;
        }
        Command::Schema { check } => {
            for (name, schema) in [
                ("catalog", schemars::schema_for!(Catalog)),
                (
                    "official-pages",
                    schemars::schema_for!(documents::PageConfig),
                ),
                (
                    "documents",
                    schemars::schema_for!(documents::DocumentCatalog),
                ),
                (
                    "document-comparisons",
                    schemars::schema_for!(Vec<documents::compare::Comparison>),
                ),
                (
                    "resolved-metadata",
                    schemars::schema_for!(documents::resolve::ResolvedMetadata),
                ),
                ("assertions", schemars::schema_for!(AssertionSet)),
                ("identities", schemars::schema_for!(IdentitySet)),
                ("profile", schemars::schema_for!(DomainMetadata)),
                (
                    "relationships",
                    schemars::schema_for!(Vec<relations::Relation>),
                ),
                ("rules", schemars::schema_for!(Vec<rules::RuleGroup>)),
                ("sources", schemars::schema_for!(discovery::SourceFile)),
                (
                    "discovery-config",
                    schemars::schema_for!(discovery::DiscoveryConfig),
                ),
                (
                    "discovery-report",
                    schemars::schema_for!(discovery::DiscoveryReport),
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
            let identities = identity::load(&cli.root)?;
            identities.validate(&registry)?;
            let document_config = documents::load_config(&cli.root)?;
            let document_catalog = documents::load(&cli.root)?;
            documents::validate_configuration(&document_config, &document_catalog, &registry)?;
            documents::validate(
                &document_catalog,
                &registry,
                matches!(command, Command::Validate { evidence: true }).then_some(cache.as_path()),
            )?;
            match command {
                Command::Documents { command } => match command {
                    DocumentCommand::Resolve { record } => {
                        print_json(&documents::resolve::resolve(
                            find_record(&registry, &record)?,
                            &identities,
                            &document_catalog,
                        )?)?;
                    }
                    DocumentCommand::List { record, vendor } => {
                        if let Some(id) = &record {
                            find_record(&registry, id)?;
                        }
                        if let Some(vendor) = &vendor {
                            ensure!(
                                document_config.pages.iter().any(|p| p.vendor == *vendor),
                                "unknown official vendor: {vendor}"
                            );
                        }
                        let matches = |source: &documents::PageSource| {
                            record.as_ref().is_none_or(|id| source.records.contains(id))
                                && vendor.as_ref().is_none_or(|v| source.vendor == *v)
                        };
                        let sources: Vec<_> = document_config
                            .pages
                            .iter()
                            .filter(|s| matches(s))
                            .collect();
                        let docs: Vec<_> = document_catalog
                            .documents
                            .iter()
                            .filter(|d| matches(&d.source))
                            .collect();
                        print_json(&json!({"sources":sources,"documents":docs}))?;
                    }
                    DocumentCommand::Check { record } => {
                        if let Some(id) = &record {
                            find_record(&registry, id)?;
                        }
                        let comparisons: Vec<_> =
                            documents::compare::compare(&document_catalog, &registry)?
                                .into_iter()
                                .filter(|c| record.as_ref().is_none_or(|id| c.record == *id))
                                .collect();
                        print_json(&comparisons)?;
                    }
                    DocumentCommand::Sync { .. } | DocumentCommand::Reparse => unreachable!(),
                },
                Command::Validate { evidence } => {
                    if cli.root.join("config/sources.yaml").try_exists()? {
                        discovery::load_sources(&cli.root)?;
                    }
                    if cli.root.join("config/discovery.yaml").try_exists()? {
                        discovery::load_config(&cli.root)?;
                    }
                    relations::derive(&registry)?;
                    println!(
                        "Validated {} records, {} identities and {} assertions{}",
                        registry.records().count(),
                        identities.models.len() + identities.interfaces.len(),
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
                    );
                }
                Command::Export { output } => {
                    let mut value = registry.export();
                    let profiles = registry
                        .records()
                        .map(|r| Ok((r.id.as_str(), domain::project(r)?)))
                        .collect::<Result<BTreeMap<_, _>>>()?;
                    value["profiles"] = serde_json::to_value(profiles)?;
                    let resolved = registry
                        .records()
                        .filter(|r| r.kind == RecordKind::Model)
                        .map(|r| {
                            Ok((
                                r.id.as_str(),
                                documents::resolve::resolve(r, &identities, &document_catalog)?,
                            ))
                        })
                        .collect::<Result<BTreeMap<_, _>>>()?;
                    value["resolved_metadata"] = serde_json::to_value(resolved)?;
                    value["identities"] = serde_json::to_value(&identities)?;
                    value["relationships"] = serde_json::to_value(relations::derive(&registry)?)?;
                    value["official_sources"] = serde_json::to_value(&document_config.pages)?;
                    value["documents"] = serde_json::to_value(&document_catalog.documents)?;
                    value["document_comparisons"] = serde_json::to_value(
                        documents::compare::compare(&document_catalog, &registry)?,
                    )?;
                    let path = cli.root.join(output);
                    store::write_bytes(&path, &json_bytes(&value)?)?;
                    println!("{}", path.display());
                }
                Command::Stats => stats(&registry, &identities, &document_catalog)?,
                Command::Query(query) => {
                    if let Some(id) = &query.identity {
                        ensure!(
                            identities.models.iter().any(|model| model.id == *id),
                            "unknown model identity: {id}"
                        );
                    }
                    let text = query.text.to_lowercase();
                    let kind = query.kind.map(|kind| match kind {
                        Kind::Model => RecordKind::Model,
                        Kind::Service => RecordKind::Service,
                    });
                    let mut records = Vec::new();
                    for record in registry.records() {
                        if query
                            .source
                            .as_ref()
                            .is_none_or(|source| record.source == *source)
                            && kind.is_none_or(|kind| record.kind == kind)
                            && (record.id.to_lowercase().contains(&text)
                                || record.name.to_lowercase().contains(&text))
                            && query.identity.as_ref().is_none_or(|id| {
                                identities
                                    .model_for_record(record.model.as_deref().unwrap_or(&record.id))
                                    .is_some_and(|model| model.id == *id)
                            })
                        {
                            let resolved = (record.kind == RecordKind::Model)
                                .then(|| {
                                    documents::resolve::resolve(
                                        record,
                                        &identities,
                                        &document_catalog,
                                    )
                                })
                                .transpose()?;
                            if query.matches(&domain::project(record)?, resolved.as_ref()) {
                                records.push(record);
                            }
                        }
                    }
                    records.sort_by(|a, b| a.id.cmp(&b.id));
                    records.truncate(query.limit);
                    print_json(&records)?;
                }
                Command::Inspect { record } => {
                    let found = find_record(&registry, &record)?;
                    let scopes = identities.scopes_for_record(&record);
                    let groups = if scopes.is_empty() {
                        vec![]
                    } else {
                        rules::query(&registry, &scopes, None, &BTreeMap::new())?
                    };
                    let catalog = registry
                        .catalogs
                        .iter()
                        .find(|catalog| catalog.source == found.source)
                        .expect("record belongs to a catalog");
                    let observed = catalog
                        .last_sync
                        .as_ref()
                        .map(|last| last.returned_ids.contains(&record));
                    let relationships: Vec<_> = relations::derive(&registry)?
                        .into_iter()
                        .filter(|r| r.from == record || r.to == record)
                        .collect();
                    let resolved =
                        documents::resolve::resolve(found, &identities, &document_catalog)?;
                    print_json(
                        &json!({"record":found,"profile":domain::project(found)?,"resolved_metadata":resolved,"identities":selected_identities(identities,Some(&record)),"relationships":relationships,"sync":{"completed_at":catalog.last_sync.as_ref().map(|s|&s.completed_at),"observed_in_last_sync":observed},"rules":{"scopes":scopes,"groups":groups},"official_sources":document_config.pages.iter().filter(|p|p.records.contains(&record)).collect::<Vec<_>>(),"documents":document_catalog.documents.iter().filter(|d|d.source.records.contains(&record)).collect::<Vec<_>>(),"document_comparisons":documents::compare::compare(&document_catalog,&registry)?.into_iter().filter(|c|c.record==record).collect::<Vec<_>>()}),
                    )?;
                }
                Command::Identities { record } => {
                    if let Some(id) = &record {
                        find_record(&registry, id)?;
                    }
                    print_json(&selected_identities(identities, record.as_deref()))?;
                }
                Command::Relations { record, limit } => {
                    if let Some(id) = &record {
                        find_record(&registry, id)?;
                    }
                    let mut relationships: Vec<_> = relations::derive(&registry)?
                        .into_iter()
                        .filter(|r| {
                            record
                                .as_ref()
                                .is_none_or(|id| r.from == *id || r.to == *id)
                        })
                        .collect();
                    let total = relationships.len();
                    relationships.truncate(limit);
                    print_json(&json!({"total":total,"relationships":relationships}))?;
                }
                Command::Rules {
                    record,
                    scope,
                    scope_kind,
                    property,
                    context,
                } => {
                    let context: BTreeMap<String, Value> = serde_json::from_str(&context).context(
                        "context must be a JSON object with literal condition paths as keys",
                    )?;
                    let mut scopes = if let Some(id) = &record {
                        find_record(&registry, id)?;
                        identities.scopes_for_record(id)
                    } else {
                        vec![]
                    };
                    if let Some(id) = scope {
                        let scope = Scope {
                            id,
                            kind: match scope_kind.expect("clap requires scope_kind") {
                                RuleScope::Model => ScopeKind::Model,
                                RuleScope::Interface => ScopeKind::Interface,
                            },
                        };
                        if record.is_some() {
                            ensure!(
                                scopes.contains(&scope),
                                "record is not explicitly bound to the requested scope"
                            );
                        } else {
                            ensure!(
                                registry
                                    .assertions
                                    .iter()
                                    .flat_map(|set| &set.assertions)
                                    .any(|a| a.scope == scope)
                                    || identities
                                        .models
                                        .iter()
                                        .any(|i| scope.kind == ScopeKind::Model && i.id == scope.id)
                                    || identities
                                        .interfaces
                                        .iter()
                                        .any(|i| scope.kind == ScopeKind::Interface
                                            && i.id == scope.id),
                                "unknown rule scope: {}",
                                scope.id
                            );
                        }
                        scopes = vec![scope];
                    }
                    if let Some(interface) = context.get("interface") {
                        ensure!(
                            scopes
                                .iter()
                                .filter(|scope| scope.kind == ScopeKind::Interface)
                                .all(|scope| interface.as_str() == Some(scope.id.as_str())),
                            "context.interface conflicts with a selected interface scope; use --scope to select the intended interface"
                        );
                    }
                    let groups = if record.is_some() && scopes.is_empty() {
                        vec![]
                    } else {
                        rules::query(&registry, &scopes, property.as_deref(), &context)?
                    };
                    print_json(
                        &json!({"record":record,"scopes":scopes,"context":context,"groups":groups}),
                    )?;
                }
                Command::Evidence { command } => {
                    let written = match command {
                        EvidenceCommand::Archive { output } => {
                            let target = cli.root.join(output);
                            documents::validate(&document_catalog, &registry, Some(&cache))?;
                            evidence::archive(&registry, &cache, &target)?
                                + documents::copy_evidence(
                                    &document_catalog,
                                    &registry,
                                    &cache,
                                    &target,
                                )?
                        }
                        EvidenceCommand::Restore { input } => {
                            let source = cli.root.join(input);
                            documents::validate(&document_catalog, &registry, Some(&source))?;
                            evidence::restore(&registry, &source, &cache)?
                                + documents::copy_evidence(
                                    &document_catalog,
                                    &registry,
                                    &source,
                                    &cache,
                                )?
                        }
                    };
                    println!("Verified referenced snapshots; wrote {written} files");
                }
                _ => unreachable!(),
            }
        }
    }
    Ok(())
}

impl Query {
    fn matches(
        &self,
        profile: &DomainMetadata,
        resolved: Option<&documents::resolve::ResolvedMetadata>,
    ) -> bool {
        // A qualified or conflicting official field deliberately blocks raw fallback.
        let official = |property: &str| -> Option<Option<&Value>> {
            let resolved = resolved?;
            let field = resolved.fields.get(property)?;
            field
                .candidates
                .iter()
                .any(|candidate| {
                    matches!(candidate, documents::resolve::Candidate::Document { .. })
                })
                .then(|| resolved.value(property))
        };
        let modality = |wanted: &Option<String>,
                        actual: &Option<domain::Derived<Vec<domain::ModalityLabel>>>,
                        property: &str| {
            wanted.as_ref().is_none_or(|wanted| {
                if let Some(value) = official(property) {
                    return value.and_then(Value::as_array).is_some_and(|labels| {
                        labels
                            .iter()
                            .any(|label| label.as_str() == Some(wanted.as_str()))
                    });
                }
                actual.as_ref().is_some_and(|labels| {
                    labels.value.iter().any(|label| {
                        label.source_label == *wanted
                            || label.canonical.is_some_and(|value| json!(value) == *wanted)
                    })
                })
            })
        };
        let capability = self.capability.map(|wanted| match wanted {
            Capability::Reasoning => ("reasoning", &profile.capabilities.reasoning),
            Capability::Thinking => ("thinking", &profile.capabilities.thinking),
            Capability::ToolCall => ("tool_call", &profile.capabilities.tool_call),
            Capability::StructuredOutput => {
                ("structured_output", &profile.capabilities.structured_output)
            }
            Capability::OpenWeights => ("open_weights", &profile.capabilities.open_weights),
            Capability::Attachment => ("attachment", &profile.capabilities.attachment),
            Capability::TemperatureControl => (
                "temperature_control",
                &profile.capabilities.temperature_control,
            ),
        });
        self.task.as_ref().is_none_or(|wanted| {
            profile.tasks.as_ref().is_some_and(|tasks| {
                tasks.labels.iter().any(|label| {
                    label.value.source_label == *wanted
                        || label
                            .value
                            .family
                            .is_some_and(|value| json!(value) == *wanted)
                })
            })
        }) && modality(&self.input, &profile.modalities.input, "input_modalities")
            && modality(
                &self.output,
                &profile.modalities.output,
                "output_modalities",
            )
            && self.min_context.is_none_or(|min| {
                if let Some(value) = official("context_window") {
                    return value
                        .and_then(Value::as_u64)
                        .is_some_and(|limit| limit >= min);
                }
                profile
                    .limits
                    .context_window
                    .as_ref()
                    .is_some_and(|limit| limit.value >= min)
            })
            && capability.is_none_or(|(property, fact)| {
                official(property).map_or_else(
                    || fact.as_ref().is_some_and(|fact| fact.value),
                    |value| value.and_then(Value::as_bool) == Some(true),
                )
            })
    }
}

fn find_record<'a>(registry: &'a Registry, id: &str) -> Result<&'a Record> {
    registry
        .records()
        .find(|record| record.id == id)
        .with_context(|| format!("unknown record: {id}"))
}

fn selected_identities(mut identities: IdentitySet, record: Option<&str>) -> IdentitySet {
    if let Some(record) = record {
        identities
            .models
            .retain(|group| group.records.iter().any(|id| id == record));
        identities
            .interfaces
            .retain(|group| group.records.iter().any(|id| id == record));
        identities.sources.retain(|id, _| {
            identities
                .models
                .iter()
                .chain(&identities.interfaces)
                .flat_map(|group| &group.evidence)
                .any(|e| e.source == *id)
        });
    }
    identities
}

fn stats(
    registry: &Registry,
    identities: &IdentitySet,
    documents: &documents::DocumentCatalog,
) -> Result<()> {
    let mut sources = BTreeMap::new();
    let mut tasks = BTreeMap::<String, usize>::new();
    let mut coverage = BTreeMap::<&str, usize>::new();
    for catalog in &registry.catalogs {
        sources.insert(&catalog.source, json!({"models":catalog.records.iter().filter(|r|r.kind == RecordKind::Model).count(),"services":catalog.records.iter().filter(|r|r.kind == RecordKind::Service).count(),"snapshots":catalog.snapshots.len(),"last_sync":catalog.last_sync.as_ref().map(|s|json!({"completed_at":s.completed_at,"returned":s.returned_ids.len(),"missing":s.missing_ids.len()}))}));
    }
    for record in registry.records() {
        let profile = domain::project(record)?;
        if let Some(labels) = &profile.tasks {
            for label in &labels.labels {
                *tasks
                    .entry(format!("{}:{}", labels.source, label.value.source_label))
                    .or_default() += 1;
            }
        }
        for (field, present) in [
            ("task", profile.tasks.is_some()),
            ("input_modalities", profile.modalities.input.is_some()),
            ("output_modalities", profile.modalities.output.is_some()),
            ("context_window", profile.limits.context_window.is_some()),
            ("parameters", profile.parameters.safetensors_total.is_some()),
            ("pricing", profile.pricing.is_some()),
        ] {
            *coverage.entry(field).or_default() += usize::from(present);
        }
    }
    let mut relationships = BTreeMap::new();
    for relation in relations::derive(registry)? {
        *relationships.entry(relation.kind).or_insert(0usize) += 1;
    }
    print_json(
        &json!({"documents":{"pages":documents.documents.len(),"failed":documents.documents.iter().filter(|d|d.error.is_some()).count(),"with_content":documents.documents.iter().filter(|d|d.content.is_some()).count(),"passages":documents.documents.iter().filter_map(|d|d.content.as_ref()).map(|c|c.extraction.passages.len()).sum::<usize>(),"claims":documents.documents.iter().filter_map(|d|d.content.as_ref()).map(|c|c.extraction.claims.len()).sum::<usize>()},"sources":sources,"task_facts":tasks,"typed_coverage":coverage,"identities":{"models":identities.models.len(),"model_records":identities.models.iter().map(|i|i.records.len()).sum::<usize>(),"interfaces":identities.interfaces.len(),"service_bindings":identities.interfaces.iter().map(|i|i.records.len()).sum::<usize>()},"relationships":relationships,"assertions":registry.assertions.iter().map(|s|s.assertions.len()).sum::<usize>(),"conflicts":registry.assertions.iter().flat_map(|s|&s.assertions).filter(|a|a.review == Review::Conflicted).count()}),
    )
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
