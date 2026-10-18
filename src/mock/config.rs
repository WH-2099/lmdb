use std::{collections::BTreeSet, fs::File, io::Read, path::Path};

use anyhow::{Context, Result, bail, ensure};
use granit_parser::{Event, Parser};
use serde_json::{Value, json};
use serde_saphyr::{DuplicateKeyPolicy, MergeKeyPolicy, NonFiniteFloatPolicy};

use super::model::{Config, ProviderProfile};

const MAX_CONFIG_BYTES: usize = 4 * 1024 * 1024;
const MAX_MODELS: usize = 4096;
const MAX_SCRIPT_STEPS: usize = 256;

pub fn load(path: Option<&Path>) -> Result<Config> {
    let text = match path {
        Some(path) => {
            let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
            ensure!(
                file.metadata()?.is_file(),
                "mock config must be a regular file"
            );
            ensure!(
                file.metadata()?.len() <= MAX_CONFIG_BYTES as u64,
                "mock config exceeds the 4 MiB limit"
            );
            let mut text = String::new();
            file.take(MAX_CONFIG_BYTES as u64 + 1)
                .read_to_string(&mut text)
                .with_context(|| format!("read {}", path.display()))?;
            text
        }
        None => include_str!("../../data/mock/providers.yaml").to_owned(),
    };
    let config = parse(&text).context("decode mock configuration")?;
    validate(&config)?;
    Ok(config)
}

fn parse(text: &str) -> Result<Config> {
    ensure!(
        text.len() <= MAX_CONFIG_BYTES,
        "mock config exceeds the 4 MiB limit"
    );
    let options = serde_saphyr::options! {
        strict_booleans: true,
        no_schema: true,
        duplicate_keys: DuplicateKeyPolicy::Error,
        merge_keys: MergeKeyPolicy::Error,
        reject_unsupported_tags: true,
        non_finite_float_policy: NonFiniteFloatPolicy::Reject,
        budget: serde_saphyr::budget! {
            max_documents: 1,
            max_anchors: 0,
            max_aliases: 0,
            max_depth: 32,
            max_nodes: 200_000,
            max_events: 800_000,
            max_total_scalar_bytes: MAX_CONFIG_BYTES,
        },
    };
    let value: Value = serde_saphyr::from_str_with_options(text, options)?;
    for event in Parser::new_from_str(text) {
        let (event, _) = event?;
        if matches!(
            event,
            Event::Scalar(_, _, _, Some(_))
                | Event::SequenceStart(_, _, Some(_))
                | Event::MappingStart(_, _, Some(_))
        ) {
            bail!("explicit YAML tags are not allowed in mock configuration");
        }
    }
    Ok(serde_json::from_value(value)?)
}

pub fn validate(config: &Config) -> Result<()> {
    ensure!(
        config.schema_version == 1,
        "unsupported mock config schema_version"
    );
    ensure!(
        !config.providers.is_empty(),
        "mock config must contain a provider"
    );
    let mut source_ids = BTreeSet::new();
    let date_format = time::format_description::parse_borrowed::<2>("[year]-[month]-[day]")?;
    for source in &config.sources {
        identifier(&source.id, "source id")?;
        ensure!(
            source_ids.insert(source.id.as_str()),
            "duplicate source {}",
            source.id
        );
        let url = reqwest::Url::parse(&source.url)
            .with_context(|| format!("invalid source URL for {}", source.id))?;
        ensure!(
            url.scheme() == "https"
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none(),
            "source {} must use an HTTPS URL without credentials",
            source.id
        );
        ensure!(
            source.checked_on.len() == 10
                && time::Date::parse(&source.checked_on, &date_format).is_ok(),
            "source {} checked_on must be a valid YYYY-MM-DD date",
            source.id
        );
        ensure!(
            !source.scope.trim().is_empty(),
            "source {} needs a scope",
            source.id
        );
    }

    let mut provider_names = BTreeSet::new();
    for profile in &config.providers {
        provider_id(&profile.id)?;
        ensure!(
            provider_names.insert(profile.id.as_str()),
            "duplicate provider {}",
            profile.id
        );
    }
    let mut model_count = 0usize;
    for profile in &config.providers {
        for alias in &profile.aliases {
            provider_id(alias)?;
            ensure!(
                provider_names.insert(alias.as_str()),
                "provider alias collision: {alias}"
            );
        }
        let protocols: BTreeSet<_> = profile.protocols.iter().copied().collect();
        ensure!(
            !protocols.is_empty() && protocols.len() == profile.protocols.len(),
            "provider {} protocols must be nonempty and unique",
            profile.id
        );
        references(&profile.sources, &source_ids, &profile.id)?;
        model_count = model_count.saturating_add(profile.models.len());
        ensure!(
            model_count <= MAX_MODELS,
            "mock config exceeds the 4096 model limit"
        );
        let mut model_ids = BTreeSet::new();
        for model in &profile.models {
            identifier(&model.id, "model id")?;
            ensure!(
                model_ids.insert(model.id.as_str()),
                "duplicate model {} in {}",
                model.id,
                profile.id
            );
            if let Some(record) = &model.record {
                identifier(record, "record id")?;
            }
            let model_protocols: BTreeSet<_> = model.protocols.iter().copied().collect();
            ensure!(
                !model_protocols.is_empty()
                    && model_protocols.len() == model.protocols.len()
                    && model_protocols.is_subset(&protocols),
                "model {} protocols must be unique and a nonempty subset of provider protocols",
                model.id
            );
            for (name, values) in [
                ("input_modalities", &model.input_modalities),
                ("output_modalities", &model.output_modalities),
            ] {
                let mut seen = BTreeSet::new();
                for value in values {
                    identifier(value, name)?;
                    ensure!(
                        seen.insert(value),
                        "model {} has duplicate {name}",
                        model.id
                    );
                }
            }
            ensure!(
                model.context_window != Some(0),
                "model {} context_window must be positive",
                model.id
            );
            ensure!(
                model.max_output_tokens != Some(0),
                "model {} max_output_tokens must be positive",
                model.id
            );
            references(&model.sources, &source_ids, &model.id)?;
        }
    }
    if let Some(script) = &config.script {
        ensure!(
            !script.steps.is_empty() && script.steps.len() <= MAX_SCRIPT_STEPS,
            "mock script must contain 1 to 256 steps"
        );
    }
    Ok(())
}

fn identifier(value: &str, name: &str) -> Result<()> {
    ensure!(
        !value.is_empty() && !value.chars().any(|c| c.is_whitespace() || c.is_control()),
        "{name} must be nonempty and contain no whitespace or control characters"
    );
    Ok(())
}

fn provider_id(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty()
            && value
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_'),
        "provider ids and aliases must use lowercase ASCII letters, digits, hyphens or underscores"
    );
    Ok(())
}

fn references(values: &[String], sources: &BTreeSet<&str>, owner: &str) -> Result<()> {
    let mut seen = BTreeSet::new();
    for source in values {
        ensure!(
            sources.contains(source.as_str()),
            "{owner} references unknown source {source}"
        );
        ensure!(
            seen.insert(source),
            "{owner} references source {source} more than once"
        );
    }
    Ok(())
}

pub fn provider<'a>(config: &'a Config, id: &str) -> Result<&'a ProviderProfile> {
    config
        .providers
        .iter()
        .find(|profile| profile.id == id || profile.aliases.iter().any(|alias| alias == id))
        .with_context(|| format!("unknown mock provider {id}"))
}

pub fn rankings(top: usize) -> Result<Value> {
    let mut snapshot: Value = serde_json::from_str(include_str!("../../data/mock/rankings.json"))?;
    let entries = snapshot["entries"]
        .as_array_mut()
        .context("ranking snapshot needs entries")?;
    let available = entries.len();
    ensure!(
        top > 0 && top <= available,
        "top must be between 1 and {available}"
    );
    entries.truncate(top);
    let unmapped: Vec<_> = entries
        .iter()
        .filter(|entry| entry["interfaces"].as_array().is_none_or(Vec::is_empty))
        .map(|entry| {
            json!({
                "rank": entry["rank"], "slug": entry["slug"], "name": entry["name"],
                "reason": entry["interface_status"],
            })
        })
        .collect();
    snapshot["requested_top"] = json!(top);
    snapshot["snapshot_entries"] = json!(available);
    snapshot["unmapped_interfaces"] = json!(unmapped);
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::model::{Fixture, Protocol, Script};

    fn custom() -> Config {
        serde_json::from_value(json!({
            "schema_version": 1, "sources": [],
            "providers": [{"id": "local", "protocols": ["openai_chat"], "models": [{
                "id": "Vendor/CaseSensitive:v1", "protocols": ["openai_chat"], "output_modalities": ["text"]
            }]}]
        })).unwrap()
    }

    #[test]
    fn builtin_models_preserve_native_and_gateway_ids_and_unknown_facts() -> Result<()> {
        let config = load(None)?;
        assert_eq!(provider(&config, "gemini")?.id, "google");
        assert_eq!(provider(&config, "kimi")?.id, "moonshotai");
        assert_eq!(provider(&config, "zai")?.id, "zhipuai");
        assert_eq!(provider(&config, "qwen")?.id, "alibaba");
        assert!(provider(&config, "unknown").is_err());
        assert!(
            provider(&config, "openai")?
                .models
                .iter()
                .any(|m| m.id == "gpt-6.1-sol")
        );
        assert!(
            provider(&config, "anthropic")?
                .models
                .iter()
                .any(|m| m.id == "claude-opus-5-5")
        );
        let gateway = provider(&config, "vercel")?
            .models
            .iter()
            .find(|m| m.id == "anthropic/claude-opus-5.5")
            .unwrap();
        assert_eq!(gateway.structured_output, None);
        assert!(gateway.behavior.is_empty());
        assert!(provider(&config, "deepseek")?.models.is_empty());
        Ok(())
    }

    #[test]
    fn custom_file_replaces_builtins_and_failed_load_does_not_change_it() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("mock.yaml");
        std::fs::write(&path, serde_json::to_vec(&custom())?)?;
        let config = load(Some(&path))?;
        assert_eq!(config.providers.len(), 1);
        assert!(provider(&config, "openai").is_err());
        assert_eq!(
            provider(&config, "local")?.models[0].id,
            "Vendor/CaseSensitive:v1"
        );
        std::fs::write(&path, "schema_version: 1\nproviders: [")?;
        assert!(load(Some(&path)).is_err());
        assert_eq!(config.providers[0].models[0].id, "Vendor/CaseSensitive:v1");
        Ok(())
    }

    #[test]
    fn rejects_ambiguous_aliases_models_and_protocols() {
        let mut config = custom();
        config.providers[0].aliases = vec!["local".into()];
        assert!(validate(&config).is_err());
        config = custom();
        config.providers.push(config.providers[0].clone());
        config.providers[1].id = "second".into();
        config.providers[0].aliases = vec!["second".into()];
        assert!(validate(&config).is_err());
        config = custom();
        let duplicate = config.providers[0].models[0].clone();
        config.providers[0].models.push(duplicate);
        assert!(validate(&config).is_err());
        config = custom();
        config.providers[0].models[0].protocols = vec![Protocol::GeminiGenerate];
        assert!(validate(&config).is_err());
    }

    #[test]
    fn rejects_invalid_source_references_urls_dates_and_input_limits() -> Result<()> {
        let mut config = custom();
        config.providers[0].sources.push("missing".into());
        assert!(validate(&config).is_err());
        for (url, date) in [
            ("http://example.org/doc", "2026-10-19"),
            ("https://key@example.org/doc", "2026-10-19"),
            ("https://example.org/doc", "2026-02-30"),
        ] {
            let mut config = custom();
            config.sources = serde_json::from_value(
                json!([{"id": "doc", "url": url, "checked_on": date, "scope": "test"}]),
            )?;
            assert!(validate(&config).is_err());
        }
        let mut config = custom();
        let source = serde_json::from_value(json!({
            "id": "doc", "url": "https://example.org/doc", "checked_on": "2026-10-19", "scope": "test"
        }))?;
        config.sources = vec![source];
        config.sources.push(config.sources[0].clone());
        assert!(validate(&config).is_err());
        let mut config = custom();
        config.providers[0].models = (0..=MAX_MODELS)
            .map(|i| {
                let mut model = custom().providers.remove(0).models.remove(0);
                model.id = format!("model-{i}");
                model
            })
            .collect();
        assert!(validate(&config).is_err());
        let mut config = custom();
        config.script = Some(Script {
            steps: vec![Fixture::default(); MAX_SCRIPT_STEPS + 1],
            repeat: false,
        });
        assert!(validate(&config).is_err());
        assert!(parse(&" ".repeat(MAX_CONFIG_BYTES + 1)).is_err());
        assert!(parse("schema_version: 1\nschema_version: 2\nsources: []\nproviders: []").is_err());
        assert!(parse("schema_version: !!int 1\nsources: []\nproviders: []").is_err());
        assert!(parse("schema_version: 1\nsources: &x []\nproviders: *x").is_err());
        Ok(())
    }

    #[test]
    fn rankings_keep_variants_gaps_and_future_coverage_distinct() -> Result<()> {
        let top20 = rankings(20)?;
        assert_eq!(top20["entries"].as_array().unwrap().len(), 20);
        assert_eq!(top20["entries"][0]["slug"], "claude-opus-5-5");
        assert_eq!(top20["entries"][2]["slug"], "claude-opus-5-5-xhigh");
        let full = top20["unmapped_interfaces"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["slug"] == "muse-wonder-full")
            .unwrap();
        assert_eq!(full["reason"], "no_catalog_service_binding");
        let top50 = rankings(50)?;
        assert_eq!(
            top50["entries"][49]["interface_status"],
            "outside_initial_top20_coverage"
        );
        assert!(rankings(0).is_err());
        assert!(rankings(usize::MAX).is_err());
        Ok(())
    }
}
