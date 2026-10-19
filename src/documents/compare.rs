use std::collections::BTreeMap;

use anyhow::{Context, Result, ensure};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::model::{Claim, ClaimScope, DocumentCatalog, Precision};
use crate::model::{Fact, Origin, Snapshot};
use crate::registry::Registry;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ComparisonStatus {
    Agreement,
    Difference,
    MissingCatalogFact,
    Conditional,
    NotComparable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Comparison {
    pub document: String,
    pub record: String,
    pub property: String,
    pub official: Claim,
    pub official_snapshot: Snapshot,
    pub official_stale: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog: Option<Fact>,
    pub catalog_origin: Origin,
    pub status: ComparisonStatus,
    pub reason: String,
}

/// Compare source declarations within their explicitly bound record, without following identities.
pub fn compare(catalog: &DocumentCatalog, registry: &Registry) -> Result<Vec<Comparison>> {
    let mut records = BTreeMap::new();
    for record in registry.records() {
        ensure!(
            records.insert(record.id.as_str(), record).is_none(),
            "duplicate record ID: {}",
            record.id
        );
    }
    let mut comparisons = Vec::new();
    for page in &catalog.documents {
        let Some(content) = &page.content else {
            continue;
        };
        for claim in &content.extraction.claims {
            ensure!(
                page.source.records.contains(&claim.record),
                "document {}: claim record {} is outside its explicit bindings",
                page.source.id,
                claim.record
            );
            let record = records.get(claim.record.as_str()).with_context(|| {
                format!(
                    "document {}: unknown claim record {}",
                    page.source.id, claim.record
                )
            })?;
            let fact = record.facts.get(&claim.property);
            let (status, reason) = assess(claim, fact, page.error.is_some());
            comparisons.push(Comparison {
                document: page.source.id.clone(),
                record: claim.record.clone(),
                property: claim.property.clone(),
                official: claim.clone(),
                official_snapshot: content.snapshot.clone(),
                official_stale: page.error.is_some(),
                source_error: page.error.clone(),
                catalog: fact.cloned(),
                catalog_origin: record.origin.clone(),
                status,
                reason: reason.to_owned(),
            });
        }
    }
    comparisons.sort_by(|left, right| {
        (&left.document, &left.record, &left.property).cmp(&(
            &right.document,
            &right.record,
            &right.property,
        ))
    });
    Ok(comparisons)
}

#[derive(Clone, Copy)]
pub(super) enum Property {
    Count(&'static str),
    Boolean,
    Modalities,
    Date,
    License,
}

pub(super) fn property(name: &str) -> Option<Property> {
    Some(match name {
        "context_window" | "max_input_tokens" | "max_output_tokens" => Property::Count("tokens"),
        "parameter_count" | "active_parameter_count" => Property::Count("parameters"),
        "reasoning"
        | "thinking"
        | "tool_call"
        | "structured_output"
        | "open_weights"
        | "attachment"
        | "temperature_control" => Property::Boolean,
        "input_modalities" | "output_modalities" => Property::Modalities,
        "knowledge_cutoff" | "release_date" => Property::Date,
        "license" => Property::License,
        _ => return None,
    })
}

fn assess(claim: &Claim, fact: Option<&Fact>, stale: bool) -> (ComparisonStatus, &'static str) {
    use ComparisonStatus::*;
    if !claim.conditions.is_empty() {
        return (Conditional, "The official claim has explicit conditions.");
    }
    if claim.scope != ClaimScope::Model {
        return (
            NotComparable,
            "The official claim does not establish model scope; interface and unspecified claims cannot be compared with model facts.",
        );
    }
    if stale {
        return (
            NotComparable,
            "The latest page retrieval failed; this claim comes from retained older content.",
        );
    }
    if claim.precision == Precision::Approximate {
        return (
            NotComparable,
            "The official claim is approximate and cannot establish exact agreement or difference.",
        );
    }
    let Some(property) = property(&claim.property) else {
        return (
            NotComparable,
            "No comparison rule is defined for this property.",
        );
    };
    if !valid_value(property, &claim.value) || !valid_unit(property, claim.unit.as_deref()) {
        return (
            NotComparable,
            "The official value or unit does not match this property's comparison rule.",
        );
    }
    let Some(fact) = fact else {
        if claim.property == "parameter_count" {
            return (
                NotComparable,
                "No explicit total parameter_count fact exists; safetensors and dtype counts have different semantics.",
            );
        }
        return (
            MissingCatalogFact,
            "The exact bound record has no raw fact for this property.",
        );
    };
    if !valid_value(property, &fact.value) || !valid_unit(property, fact.unit.as_deref()) {
        return (
            NotComparable,
            "The catalog value or unit does not match this property's comparison rule.",
        );
    }
    let equal = match equal_values(property, &claim.value, &fact.value) {
        Ok(equal) => equal,
        Err(reason) => return (NotComparable, reason),
    };
    if equal {
        (
            Agreement,
            "The exact source values agree under this property's comparison rule.",
        )
    } else {
        (
            Difference,
            "The source declaration values differ; this does not determine which declaration is correct.",
        )
    }
}

pub(super) fn equal_values(
    property: Property,
    left: &Value,
    right: &Value,
) -> std::result::Result<bool, &'static str> {
    Ok(match property {
        Property::Count(_) | Property::Boolean => left == right,
        Property::Modalities => string_set(left) == string_set(right),
        Property::Date => {
            if date_precision(left) != date_precision(right) {
                return Err("The source dates have different precision.");
            }
            left == right
        }
        Property::License => match (left, right) {
            (Value::String(_), Value::String(_)) => left == right,
            (Value::Array(_), Value::Array(_)) => string_set(left) == string_set(right),
            _ => {
                return Err("The license declarations use different value shapes.");
            }
        },
    })
}

pub(super) fn valid_unit(property: Property, unit: Option<&str>) -> bool {
    match property {
        Property::Count(expected) => unit == Some(expected),
        _ => unit.is_none(),
    }
}

pub(super) fn valid_value(property: Property, value: &Value) -> bool {
    match property {
        Property::Count(_) => value.as_u64().is_some(),
        Property::Boolean => value.is_boolean(),
        Property::Modalities => string_set(value).is_some_and(|labels| {
            labels
                .iter()
                .all(|label| matches!(*label, "text" | "image" | "audio" | "video" | "embedding"))
        }),
        Property::Date => date_precision(value).is_some(),
        Property::License => {
            value.as_str().is_some_and(|value| !value.trim().is_empty())
                || string_set(value).is_some_and(|labels| {
                    !labels.is_empty() && labels.iter().all(|label| !label.trim().is_empty())
                })
        }
    }
}

fn string_set(value: &Value) -> Option<Vec<&str>> {
    let mut labels = value
        .as_array()?
        .iter()
        .map(Value::as_str)
        .collect::<Option<Vec<_>>>()?;
    labels.sort_unstable();
    labels.dedup();
    Some(labels)
}

fn date_precision(value: &Value) -> Option<usize> {
    let date = value.as_str()?;
    let parts = date.split('-').collect::<Vec<_>>();
    if !matches!(parts.len(), 2 | 3)
        || parts[0].len() != 4
        || parts[1..].iter().any(|part| part.len() != 2)
        || !parts
            .iter()
            .all(|part| part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return None;
    }
    let year = parts[0].parse().ok()?;
    let month = time::Month::try_from(parts[1].parse::<u8>().ok()?).ok()?;
    let day = parts.get(2).map_or(Some(1), |day| day.parse().ok())?;
    time::Date::from_calendar_date(year, month, day).ok()?;
    Some(parts.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::model::{
        ClaimScope, DocumentKind, Extraction, Format, PageContent, PageDocument, PageEvidence,
        PageSource,
    };
    use crate::model::{Catalog, Record, RecordKind, SCHEMA_VERSION};
    use serde_json::json;

    fn claim(property: &str, value: Value, unit: Option<&str>) -> Claim {
        Claim {
            record: "modelsdev:model:lab/model".into(),
            property: property.into(),
            value,
            unit: unit.map(str::to_owned),
            precision: Precision::Exact,
            scope: ClaimScope::Model,
            conditions: Vec::new(),
            evidence: PageEvidence {
                locator: "#specs td:nth-child(2)".into(),
                excerpt: "Exact specification from the official model page.".into(),
            },
        }
    }

    fn fact(value: Value, unit: Option<&str>) -> Fact {
        Fact {
            value,
            pointer: "/limit/context".into(),
            unit: unit.map(str::to_owned),
        }
    }

    fn registry(facts: &[(&str, Fact)]) -> Registry {
        Registry {
            catalogs: vec![Catalog {
                schema_version: SCHEMA_VERSION,
                source: "modelsdev".into(),
                last_sync: None,
                snapshots: Vec::new(),
                records: vec![Record {
                    schema_version: SCHEMA_VERSION,
                    kind: RecordKind::Model,
                    id: "modelsdev:model:lab/model".into(),
                    source: "modelsdev".into(),
                    upstream_id: "lab/model".into(),
                    name: "Model".into(),
                    model: None,
                    origin: Origin {
                        snapshot: "modelsdev:raw".into(),
                        pointer: "/lab~1model".into(),
                    },
                    facts: facts
                        .iter()
                        .map(|(key, value)| ((*key).into(), value.clone()))
                        .collect(),
                }],
            }],
            assertions: Vec::new(),
        }
    }

    fn documents(claims: Vec<Claim>) -> DocumentCatalog {
        DocumentCatalog {
            schema_version: SCHEMA_VERSION,
            documents: vec![PageDocument {
                error_kind: None,
                failed_snapshot: None,
                source: PageSource {
                    repository: None,
                    id: "official-specs".into(),
                    vendor: "lab".into(),
                    kind: DocumentKind::ModelPage,
                    url: "https://example.com/model".into(),
                    format: Format::Html,
                    records: vec!["modelsdev:model:lab/model".into()],
                },
                checked_at: "2026-10-16T00:00:00Z".into(),
                error: None,
                content: Some(PageContent {
                    snapshot: Snapshot {
                        schema_version: SCHEMA_VERSION,
                        id: "official:page".into(),
                        source: "official".into(),
                        url: "https://example.com/model".into(),
                        retrieved_at: "2026-10-16T00:00:00Z".into(),
                        sha256: "a".repeat(64),
                        upstream_revision: None,
                    },
                    content_type: Some("text/html".into()),
                    extraction: Extraction {
                        extractor_version: 1,
                        title: None,
                        published_at: None,
                        canonical_url: None,
                        links: Vec::new(),
                        catalog_entries: Vec::new(),
                        passages: Vec::new(),
                        claims,
                        warnings: Vec::new(),
                    },
                }),
            }],
        }
    }

    #[test]
    fn compares_exact_claims_and_retains_both_sources_evidence() {
        let registry = registry(&[
            ("context_window", fact(json!(0), Some("tokens"))),
            ("max_output_tokens", fact(json!(4096), Some("tokens"))),
            ("input_modalities", fact(json!(["image", "text"]), None)),
            ("reasoning", fact(json!(false), None)),
        ]);
        let documents = documents(vec![
            claim("context_window", json!(0), Some("tokens")),
            claim("max_output_tokens", json!(8192), Some("tokens")),
            claim("input_modalities", json!(["text", "image"]), None),
            claim("reasoning", json!(false), None),
            claim("max_input_tokens", json!(128000), Some("tokens")),
        ]);
        let result = compare(&documents, &registry).unwrap();
        let by_property = result
            .iter()
            .map(|item| (item.property.as_str(), item))
            .collect::<BTreeMap<_, _>>();
        for property in ["context_window", "input_modalities", "reasoning"] {
            assert_eq!(by_property[property].status, ComparisonStatus::Agreement);
        }
        assert_eq!(
            by_property["max_output_tokens"].status,
            ComparisonStatus::Difference
        );
        assert_eq!(
            by_property["max_input_tokens"].status,
            ComparisonStatus::MissingCatalogFact
        );
        let modalities = by_property["input_modalities"];
        assert_eq!(modalities.official.value, json!(["text", "image"]));
        assert_eq!(
            modalities.catalog.as_ref().unwrap().value,
            json!(["image", "text"])
        );
        assert_eq!(modalities.official_snapshot.id, "official:page");
        assert_eq!(modalities.catalog_origin.snapshot, "modelsdev:raw");
        assert!(!modalities.official_stale);
    }

    #[test]
    fn conditions_precede_approximation_and_stale_content_cannot_agree() {
        let registry = registry(&[("context_window", fact(json!(128000), Some("tokens")))]);
        let mut conditional = claim("context_window", json!(128000), Some("tokens"));
        conditional
            .conditions
            .push("Requires the beta header".into());
        conditional.precision = Precision::Approximate;
        let mut approximate = claim("context_window", json!(128000), Some("tokens"));
        approximate.precision = Precision::Approximate;
        let mut documents = documents(vec![
            conditional,
            approximate,
            claim("context_window", json!(128000), Some("tokens")),
        ]);
        let result = compare(&documents, &registry).unwrap();
        assert_eq!(result[0].status, ComparisonStatus::Conditional);
        assert_eq!(result[1].status, ComparisonStatus::NotComparable);
        assert_eq!(result[2].status, ComparisonStatus::Agreement);
        documents.documents[0].error = Some("HTTP 503".into());
        let stale = compare(&documents, &registry).unwrap();
        assert_eq!(stale[0].status, ComparisonStatus::Conditional);
        assert_eq!(stale[2].status, ComparisonStatus::NotComparable);
        assert!(stale.iter().all(|comparison| comparison.official_stale));
        assert_eq!(stale[2].source_error.as_deref(), Some("HTTP 503"));
    }

    #[test]
    fn interface_and_unspecified_claims_cannot_agree_with_model_facts() {
        let registry = registry(&[("context_window", fact(json!(128000), Some("tokens")))]);
        let mut interface = claim("context_window", json!(128000), Some("tokens"));
        interface.scope = ClaimScope::Interface {
            id: "https://example.com/api".into(),
        };
        let mut unspecified = interface.clone();
        unspecified.scope = ClaimScope::Unspecified;
        let mut conditional = interface.clone();
        conditional
            .conditions
            .push("Requires the beta header".into());
        let compared = compare(
            &documents(vec![interface, unspecified, conditional]),
            &registry,
        )
        .unwrap();
        assert_eq!(compared[0].status, ComparisonStatus::NotComparable);
        assert_eq!(compared[1].status, ComparisonStatus::NotComparable);
        assert_eq!(compared[2].status, ComparisonStatus::Conditional);
        assert!(compared[0].reason.contains("scope"));
    }

    #[test]
    fn units_value_shapes_and_date_precision_must_match() {
        let cases = [
            (
                "context_window",
                json!(128000),
                Some("tokens"),
                json!(128000),
                None,
            ),
            (
                "context_window",
                json!(128000),
                None,
                json!(128000),
                Some("tokens"),
            ),
            (
                "context_window",
                json!(128000),
                Some("tokens"),
                json!(128000),
                Some("bytes"),
            ),
            (
                "context_window",
                json!(128000),
                Some("tokens"),
                json!("128000"),
                Some("tokens"),
            ),
            (
                "context_window",
                json!(128000),
                Some("bytes"),
                json!(128000),
                Some("bytes"),
            ),
            (
                "knowledge_cutoff",
                json!("2024-08"),
                None,
                json!("2024-08-01"),
                None,
            ),
            (
                "release_date",
                json!("2025-02-30"),
                None,
                json!("2025-02-30"),
                None,
            ),
            (
                "input_modalities",
                json!(["audio"]),
                None,
                json!(["speech"]),
                None,
            ),
            ("license", json!("MIT"), None, json!(["MIT"]), None),
        ];
        for (property, official, official_unit, catalog, catalog_unit) in cases {
            let registry = registry(&[(property, fact(catalog, catalog_unit))]);
            let result = compare(
                &documents(vec![claim(property, official, official_unit)]),
                &registry,
            )
            .unwrap();
            assert_eq!(
                result[0].status,
                ComparisonStatus::NotComparable,
                "{property}: {}",
                result[0].reason
            );
        }
        for (property, value) in [
            ("knowledge_cutoff", json!("2024-08")),
            ("release_date", json!("2025-02-28")),
            ("license", json!(["MIT", "Apache-2.0"])),
        ] {
            let registry = registry(&[(property, fact(value.clone(), None))]);
            assert_eq!(
                compare(&documents(vec![claim(property, value, None)]), &registry).unwrap()[0]
                    .status,
                ComparisonStatus::Agreement
            );
        }
    }

    #[test]
    fn parameter_totals_and_reasoning_require_their_own_explicit_facts() {
        let mut registry = registry(&[
            (
                "safetensors_parameter_count",
                fact(json!(8_190_735_360u64), Some("parameters")),
            ),
            ("supported_parameters", fact(json!(["reasoning"]), None)),
        ]);
        let documents = documents(vec![
            claim(
                "parameter_count",
                json!(8_000_000_000u64),
                Some("parameters"),
            ),
            claim("reasoning", json!(true), None),
        ]);
        let result = compare(&documents, &registry).unwrap();
        assert_eq!(result[0].status, ComparisonStatus::NotComparable);
        assert!(result[0].catalog.is_none());
        assert_eq!(result[1].status, ComparisonStatus::MissingCatalogFact);
        registry.catalogs[0].records[0].facts.insert(
            "parameter_count".into(),
            fact(json!(8_000_000_000u64), Some("parameters")),
        );
        assert_eq!(
            compare(&documents, &registry).unwrap()[0].status,
            ComparisonStatus::Agreement
        );
    }

    #[test]
    fn record_scope_does_not_follow_models_aliases_or_other_sources() {
        let mut registry = registry(&[("context_window", fact(json!(128000), Some("tokens")))]);
        let mut service = registry.catalogs[0].records[0].clone();
        service.kind = RecordKind::Service;
        service.id = "modelsdev:service:lab/model".into();
        service.model = Some("modelsdev:model:lab/model".into());
        service.facts.clear();
        registry.catalogs[0].records.push(service);
        let mut documents = documents(vec![claim("context_window", json!(128000), Some("tokens"))]);
        documents.documents[0].source.records = vec!["modelsdev:service:lab/model".into()];
        documents.documents[0]
            .content
            .as_mut()
            .unwrap()
            .extraction
            .claims[0]
            .record = "modelsdev:service:lab/model".into();
        assert_eq!(
            compare(&documents, &registry).unwrap()[0].status,
            ComparisonStatus::MissingCatalogFact
        );
        documents.documents[0]
            .content
            .as_mut()
            .unwrap()
            .extraction
            .claims[0]
            .record = "huggingface:model:lab/model".into();
        assert!(compare(&documents, &registry).is_err());
        documents.documents[0].source.records = vec!["huggingface:model:lab/model".into()];
        assert!(compare(&documents, &registry).is_err());
    }
}
