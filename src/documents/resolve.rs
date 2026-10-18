use std::collections::BTreeMap;

use anyhow::{Result, ensure};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    compare,
    model::{Claim, ClaimScope, DocumentCatalog, DocumentKind, Precision},
};
use crate::{
    identity::IdentitySet,
    model::{Fact, Origin, Record, RecordKind, Snapshot},
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResolvedMetadata {
    pub record: String,
    pub fields: BTreeMap<String, Resolution>,
    pub scoped_claims: Vec<Candidate>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Resolution {
    pub status: ResolutionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<usize>,
    pub candidates: Vec<Candidate>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionStatus {
    Selected,
    Qualified,
    Conflict,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Candidate {
    Catalog {
        record: String,
        origin: Origin,
        fact: Fact,
    },
    Document {
        document: String,
        kind: DocumentKind,
        snapshot: Snapshot,
        claim: Box<Claim>,
        stale: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        source_error: Option<String>,
    },
}

impl ResolvedMetadata {
    /// Only selected, exact, unconditional values are suitable for query filters.
    pub fn value(&self, property: &str) -> Option<&Value> {
        let field = self.fields.get(property)?;
        if field.status != ResolutionStatus::Selected {
            return None;
        }
        let candidate = field.candidates.get(field.selected?)?;
        candidate.eligible().then(|| candidate.value())
    }
}

impl Candidate {
    pub fn value(&self) -> &Value {
        match self {
            Self::Catalog { fact, .. } => &fact.value,
            Self::Document { claim, .. } => &claim.value,
        }
    }

    fn unit(&self) -> Option<&str> {
        match self {
            Self::Catalog { fact, .. } => fact.unit.as_deref(),
            Self::Document { claim, .. } => claim.unit.as_deref(),
        }
    }

    fn rank(&self) -> u8 {
        match self {
            Self::Catalog { .. } => 0,
            Self::Document {
                kind: DocumentKind::ReleaseBlog,
                ..
            } => 1,
            Self::Document { .. } => 2,
        }
    }

    fn eligible(&self) -> bool {
        match self {
            Self::Catalog { fact, .. } => !fact.value.is_null(),
            Self::Document { claim, stale, .. } => {
                claim.scope == ClaimScope::Model
                    && !stale
                    && claim.precision == Precision::Exact
                    && claim.conditions.is_empty()
                    && compare::property(&claim.property).map_or_else(
                        || !claim.value.is_null(),
                        |property| {
                            compare::valid_value(property, &claim.value)
                                && compare::valid_unit(property, claim.unit.as_deref())
                        },
                    )
            }
        }
    }

    fn sort_key(&self) -> (&str, &str, &str, &str) {
        match self {
            Self::Catalog {
                record,
                origin,
                fact,
            } => (&origin.snapshot, record, "", &fact.pointer),
            Self::Document {
                document,
                snapshot,
                claim,
                ..
            } => (
                &snapshot.source,
                &claim.record,
                document,
                &claim.evidence.locator,
            ),
        }
    }
}

/// Resolve validated model bindings without changing source records or their evidence.
pub fn resolve(
    record: &Record,
    identities: &IdentitySet,
    documents: &DocumentCatalog,
) -> Result<ResolvedMetadata> {
    let mut fields = BTreeMap::<String, Vec<Candidate>>::new();
    for (property, fact) in &record.facts {
        fields
            .entry(property.clone())
            .or_default()
            .push(Candidate::Catalog {
                record: record.id.clone(),
                origin: record.origin.clone(),
                fact: fact.clone(),
            });
    }
    let mut scoped_claims = Vec::new();
    if record.kind == RecordKind::Model {
        let identity = identities.model_for_record(&record.id);
        for page in &documents.documents {
            let Some(content) = &page.content else {
                continue;
            };
            for claim in &content.extraction.claims {
                let direct = claim.record == record.id;
                let model = claim.scope == ClaimScope::Model;
                let related =
                    model && identity.is_some_and(|group| group.records.contains(&claim.record));
                if !direct && !related {
                    continue;
                }
                ensure!(
                    page.source.records.contains(&claim.record),
                    "document {}: claim record {} is outside its explicit bindings",
                    page.source.id,
                    claim.record
                );
                let candidate = Candidate::Document {
                    document: page.source.id.clone(),
                    kind: page.source.kind,
                    snapshot: content.snapshot.clone(),
                    claim: Box::new(claim.clone()),
                    stale: page.error.is_some(),
                    source_error: page.error.clone(),
                };
                if model {
                    fields
                        .entry(claim.property.clone())
                        .or_default()
                        .push(candidate);
                } else {
                    scoped_claims.push(candidate);
                }
            }
        }
    }
    sort_candidates(&mut scoped_claims)?;
    let fields = fields
        .into_iter()
        .map(|(property, candidates)| Ok((property.clone(), resolve_field(&property, candidates)?)))
        .collect::<Result<_>>()?;
    Ok(ResolvedMetadata {
        record: record.id.clone(),
        fields,
        scoped_claims,
    })
}

fn sort_candidates(candidates: &mut Vec<Candidate>) -> Result<()> {
    let mut keyed = candidates
        .drain(..)
        .map(|candidate| Ok((serde_json::to_string(&candidate)?, candidate)))
        .collect::<Result<Vec<_>>>()?;
    keyed.sort_by(|(left_json, left), (right_json, right)| {
        left.sort_key()
            .cmp(&right.sort_key())
            .then_with(|| left_json.cmp(right_json))
    });
    candidates.extend(keyed.into_iter().map(|(_, candidate)| candidate));
    candidates.dedup();
    Ok(())
}

fn resolve_field(property: &str, mut candidates: Vec<Candidate>) -> Result<Resolution> {
    sort_candidates(&mut candidates)?;
    let highest = candidates.iter().map(Candidate::rank).max();
    let eligible = candidates
        .iter()
        .enumerate()
        .filter(|(_, candidate)| Some(candidate.rank()) == highest && candidate.eligible())
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let mut selected = eligible.first().copied();
    let mut status = if selected.is_some() {
        ResolutionStatus::Selected
    } else if candidates.iter().any(|candidate| candidate.rank() > 0) {
        ResolutionStatus::Qualified
    } else {
        ResolutionStatus::Unavailable
    };
    for (offset, &left) in eligible.iter().enumerate() {
        for &right in &eligible[offset + 1..] {
            let left = &candidates[left];
            let right = &candidates[right];
            let equal = if left.unit() != right.unit() {
                None
            } else if let Some(property) = compare::property(property) {
                compare::equal_values(property, left.value(), right.value()).ok()
            } else {
                Some(left.value() == right.value())
            };
            match equal {
                Some(false) => status = ResolutionStatus::Conflict,
                None if status != ResolutionStatus::Conflict => {
                    status = ResolutionStatus::Qualified;
                }
                _ => {}
            }
        }
    }
    if status != ResolutionStatus::Selected {
        selected = None;
    }
    Ok(Resolution {
        status,
        selected,
        candidates,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        documents::model::{
            Extraction, Format, PageContent, PageDocument, PageEvidence, PageSource,
        },
        identity::Identity,
    };
    use serde_json::json;

    fn record(id: &str) -> Record {
        Record {
            schema_version: 1,
            kind: RecordKind::Model,
            id: id.into(),
            source: "catalog".into(),
            upstream_id: "model".into(),
            name: "Model".into(),
            model: None,
            origin: Origin {
                snapshot: "catalog:snapshot".into(),
                pointer: "/data/0".into(),
            },
            facts: BTreeMap::from([(
                "context_window".into(),
                Fact {
                    value: json!(32768),
                    pointer: "/data/0/context_length".into(),
                    unit: Some("tokens".into()),
                },
            )]),
        }
    }

    fn claim(record: &str, property: &str, value: Value) -> Claim {
        Claim {
            record: record.into(),
            property: property.into(),
            value,
            unit: match property {
                "context_window" => Some("tokens".into()),
                "parameter_count" | "active_parameter_count" => Some("parameters".into()),
                _ => None,
            },
            precision: Precision::Exact,
            scope: ClaimScope::Model,
            conditions: Vec::new(),
            evidence: PageEvidence {
                locator: "#specs".into(),
                excerpt: "Exact specification from the original model card.".into(),
            },
        }
    }

    fn page(id: &str, kind: DocumentKind, claims: Vec<Claim>) -> PageDocument {
        PageDocument {
            source: PageSource {
                id: id.into(),
                vendor: "lab".into(),
                kind,
                url: format!("https://example.com/{id}"),
                format: Format::Html,
                records: claims.iter().map(|claim| claim.record.clone()).collect(),
            },
            checked_at: "2026-10-16T00:00:00Z".into(),
            error: None,
            content: Some(PageContent {
                snapshot: Snapshot {
                    schema_version: 1,
                    id: format!("official:{id}"),
                    source: "official".into(),
                    url: format!("https://example.com/{id}"),
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
        }
    }

    fn documents(pages: Vec<PageDocument>) -> DocumentCatalog {
        DocumentCatalog {
            schema_version: 1,
            documents: pages,
        }
    }

    fn identities() -> IdentitySet {
        IdentitySet {
            models: vec![Identity {
                id: "lab/release".into(),
                name: "Release".into(),
                records: vec!["a:model:release".into(), "b:model:release".into()],
                evidence: Vec::new(),
            }],
            ..IdentitySet::default()
        }
    }

    #[test]
    fn official_card_overrides_catalog_without_mutating_raw_evidence() {
        let record = record("a:model:release");
        let before = record.clone();
        let documents = documents(vec![page(
            "card",
            DocumentKind::ModelCard,
            vec![claim(&record.id, "context_window", json!(131072))],
        )]);
        let resolved = resolve(&record, &identities(), &documents).unwrap();
        assert_eq!(resolved.value("context_window"), Some(&json!(131072)));
        assert_eq!(record, before);
        let field = &resolved.fields["context_window"];
        assert_eq!(field.candidates.len(), 2);
        assert!(field.candidates.iter().any(|candidate| matches!(candidate,
            Candidate::Catalog { origin, fact, .. }
                if origin == &record.origin && fact == &record.facts["context_window"])));
        let serialized = serde_json::to_value(&resolved).unwrap();
        assert_eq!(
            serde_json::from_value::<ResolvedMetadata>(serialized).unwrap(),
            resolved
        );
    }

    #[test]
    fn model_identity_propagates_original_claim_but_services_keep_own_limits() {
        let record = record("b:model:release");
        let original = claim("a:model:release", "context_window", json!(131072));
        let documents = documents(vec![page(
            "card",
            DocumentKind::ModelCard,
            vec![original.clone()],
        )]);
        let resolved = resolve(&record, &identities(), &documents).unwrap();
        assert_eq!(resolved.value("context_window"), Some(&json!(131072)));
        assert!(resolved.fields["context_window"].candidates.iter().any(
            |candidate| matches!(candidate, Candidate::Document { claim, .. } if claim.as_ref() == &original)
        ));
        let mut service = record.clone();
        service.id = "host:service:release".into();
        service.kind = RecordKind::Service;
        service.model = Some(record.id);
        let resolved = resolve(&service, &identities(), &documents).unwrap();
        assert_eq!(resolved.value("context_window"), Some(&json!(32768)));
        assert_eq!(resolved.fields["context_window"].candidates.len(), 1);
        assert!(resolved.scoped_claims.is_empty());
    }

    #[test]
    fn interface_and_unspecified_claims_stay_on_their_exact_record() {
        let record = record("a:model:release");
        let mut endpoint = claim(&record.id, "context_window", json!(65536));
        endpoint.scope = ClaimScope::Interface {
            id: "lab/api".into(),
        };
        let mut unspecified = claim(&record.id, "context_window", json!(262144));
        unspecified.scope = ClaimScope::Unspecified;
        let documents = documents(vec![page(
            "specs",
            DocumentKind::ModelPage,
            vec![
                endpoint,
                unspecified,
                claim(&record.id, "context_window", json!(131072)),
            ],
        )]);
        let resolved = resolve(&record, &identities(), &documents).unwrap();
        assert_eq!(resolved.value("context_window"), Some(&json!(131072)));
        assert_eq!(resolved.scoped_claims.len(), 2);
        let mut peer = record;
        peer.id = "b:model:release".into();
        let resolved = resolve(&peer, &identities(), &documents).unwrap();
        assert!(resolved.scoped_claims.is_empty());
        assert_eq!(resolved.value("context_window"), Some(&json!(131072)));
    }

    #[test]
    fn conflicting_top_authorities_do_not_fall_back_to_blog_or_catalog() {
        let record = record("a:model:release");
        let documents = documents(vec![
            page(
                "card",
                DocumentKind::ModelCard,
                vec![claim(&record.id, "context_window", json!(32768))],
            ),
            page(
                "specs",
                DocumentKind::Documentation,
                vec![claim(&record.id, "context_window", json!(65536))],
            ),
            page(
                "blog",
                DocumentKind::ReleaseBlog,
                vec![claim(&record.id, "context_window", json!(32768))],
            ),
        ]);
        let resolved = resolve(&record, &identities(), &documents).unwrap();
        assert_eq!(
            resolved.fields["context_window"].status,
            ResolutionStatus::Conflict
        );
        assert_eq!(resolved.value("context_window"), None);
        assert_eq!(resolved.fields["context_window"].selected, None);
        assert_eq!(resolved.fields["context_window"].candidates.len(), 4);
    }

    #[test]
    fn qualified_authority_blocks_lower_values_but_allows_same_rank_exact_defaults() {
        let record = record("a:model:release");
        for qualifier in ["approximate", "conditional", "stale", "unit", "value"] {
            let mut official = claim(&record.id, "context_window", json!(131072));
            match qualifier {
                "approximate" => official.precision = Precision::Approximate,
                "conditional" => official.conditions.push("Requires YaRN".into()),
                "unit" => official.unit = Some("characters".into()),
                "value" => official.value = json!("131072"),
                _ => {}
            }
            let mut card = page("card", DocumentKind::ModelCard, vec![official]);
            if qualifier == "stale" {
                card.error = Some("HTTP 503".into());
            }
            let mut documents = documents(vec![
                card,
                page(
                    "blog",
                    DocumentKind::ReleaseBlog,
                    vec![claim(&record.id, "context_window", json!(65536))],
                ),
            ]);
            let resolved = resolve(&record, &identities(), &documents).unwrap();
            assert_eq!(
                resolved.fields["context_window"].status,
                ResolutionStatus::Qualified,
                "{qualifier}"
            );
            assert_eq!(resolved.value("context_window"), None, "{qualifier}");
            documents.documents.push(page(
                "specs",
                DocumentKind::ModelPage,
                vec![claim(&record.id, "context_window", json!(32768))],
            ));
            let resolved = resolve(&record, &identities(), &documents).unwrap();
            assert_eq!(
                resolved.value("context_window"),
                Some(&json!(32768)),
                "{qualifier}"
            );
        }
    }

    #[test]
    fn current_specs_outrank_launch_blog_without_crossing_release_bindings() {
        let record = record("a:model:release");
        let documents = documents(vec![
            page(
                "blog",
                DocumentKind::ReleaseBlog,
                vec![claim(&record.id, "context_window", json!(65536))],
            ),
            page(
                "specs",
                DocumentKind::ModelPage,
                vec![claim(&record.id, "context_window", json!(131072))],
            ),
            page(
                "new-release",
                DocumentKind::ModelPage,
                vec![claim(
                    "a:model:new-release",
                    "context_window",
                    json!(262144),
                )],
            ),
        ]);
        let resolved = resolve(&record, &identities(), &documents).unwrap();
        assert_eq!(resolved.value("context_window"), Some(&json!(131072)));
        assert_eq!(resolved.fields["context_window"].candidates.len(), 3);
    }

    #[test]
    fn ordering_duplicates_and_fetch_dates_do_not_decide_authority() {
        let record = record("a:model:release");
        let mut first = page(
            "a-card",
            DocumentKind::ModelCard,
            vec![
                claim(&record.id, "input_modalities", json!(["image", "text"])),
                claim(&record.id, "license", json!(["apache-2.0", "mit"])),
            ],
        );
        first.content.as_mut().unwrap().snapshot.retrieved_at = "2020-01-01T00:00:00Z".into();
        let second = page(
            "z-card",
            DocumentKind::ModelCard,
            vec![
                claim(
                    &record.id,
                    "input_modalities",
                    json!(["text", "image", "text"]),
                ),
                claim(&record.id, "license", json!(["mit", "apache-2.0"])),
            ],
        );
        let mut documents = documents(vec![first, second]);
        let original = resolve(&record, &identities(), &documents).unwrap();
        assert_eq!(
            original.fields["input_modalities"].status,
            ResolutionStatus::Selected
        );
        assert_eq!(
            original.fields["license"].status,
            ResolutionStatus::Selected
        );
        documents.documents.reverse();
        for document in &mut documents.documents {
            let claims = &mut document.content.as_mut().unwrap().extraction.claims;
            claims.reverse();
            claims.push(claims[0].clone());
        }
        assert_eq!(
            resolve(&record, &identities(), &documents).unwrap(),
            original
        );
    }

    #[test]
    fn rich_fields_and_total_counts_resolve_without_inventing_safetensors_equivalence() {
        let mut record = record("a:model:release");
        record.facts.insert(
            "safetensors_parameter_count".into(),
            Fact {
                value: json!(999),
                pointer: "/safetensors/total".into(),
                unit: Some("parameters".into()),
            },
        );
        let documents = documents(vec![page(
            "card",
            DocumentKind::ModelCard,
            vec![
                claim(&record.id, "parameter_count", json!(1000)),
                claim(&record.id, "active_parameter_count", json!(100)),
                claim(
                    &record.id,
                    "architecture",
                    json!({"type":"moe","experts":8}),
                ),
                claim(&record.id, "training_languages", json!(["en", "zh"])),
                claim(&record.id, "license_name", json!("Model License")),
            ],
        )]);
        let resolved = resolve(&record, &identities(), &documents).unwrap();
        assert_eq!(resolved.value("parameter_count"), Some(&json!(1000)));
        assert_eq!(resolved.value("active_parameter_count"), Some(&json!(100)));
        assert_eq!(
            resolved.value("safetensors_parameter_count"),
            Some(&json!(999))
        );
        assert_eq!(
            resolved.value("architecture"),
            Some(&json!({"type":"moe","experts":8}))
        );
        assert_eq!(
            resolved.value("training_languages"),
            Some(&json!(["en", "zh"]))
        );
        assert_eq!(
            resolved.value("license_name"),
            Some(&json!("Model License"))
        );
    }

    #[test]
    fn differing_date_precision_and_license_shapes_remain_qualified() {
        let record = record("a:model:release");
        let documents = documents(vec![page(
            "card",
            DocumentKind::ModelCard,
            vec![
                claim(&record.id, "release_date", json!("2025-01")),
                claim(&record.id, "release_date", json!("2025-01-01")),
                claim(&record.id, "license", json!("mit")),
                claim(&record.id, "license", json!(["mit"])),
            ],
        )]);
        let resolved = resolve(&record, &identities(), &documents).unwrap();
        for property in ["release_date", "license"] {
            assert_eq!(
                resolved.fields[property].status,
                ResolutionStatus::Qualified
            );
            assert_eq!(resolved.value(property), None);
        }
    }

    #[test]
    fn propagation_cannot_escape_the_original_document_binding() {
        let record = record("b:model:release");
        let mut card = page(
            "card",
            DocumentKind::ModelCard,
            vec![claim("a:model:release", "context_window", json!(131072))],
        );
        card.source.records = vec!["a:model:other".into()];
        assert!(resolve(&record, &identities(), &documents(vec![card])).is_err());
    }
}
