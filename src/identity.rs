use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use anyhow::{Context, Result, ensure};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    assertion::{self, DocumentSource, Evidence, Scope, ScopeKind},
    model::RecordKind,
    registry::Registry,
    store,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IdentitySet {
    #[schemars(range(min = 1, max = 1))]
    pub schema_version: u32,
    pub sources: BTreeMap<String, DocumentSource>,
    pub models: Vec<Identity>,
    pub interfaces: Vec<Identity>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub id: String,
    pub name: String,
    pub records: Vec<String>,
    pub evidence: Vec<Evidence>,
}

impl Default for IdentitySet {
    fn default() -> Self {
        Self {
            schema_version: 1,
            sources: BTreeMap::new(),
            models: vec![],
            interfaces: vec![],
        }
    }
}

pub fn load(root: &Path) -> Result<IdentitySet> {
    let path = root.join("data/identities/core.yaml");
    if path
        .try_exists()
        .with_context(|| format!("inspect {}", path.display()))?
    {
        store::read_yaml(&path)
    } else {
        Ok(IdentitySet::default())
    }
}

impl IdentitySet {
    pub fn validate(&self, registry: &Registry) -> Result<()> {
        ensure!(
            self.schema_version == 1,
            "unsupported identity schema version"
        );
        assertion::validate_sources(&self.sources)?;
        let records: BTreeMap<_, _> = registry
            .records()
            .map(|record| (record.id.as_str(), record))
            .collect();
        let mut model_members = BTreeSet::new();
        for (kind, groups) in [
            (ScopeKind::Model, &self.models),
            (ScopeKind::Interface, &self.interfaces),
        ] {
            let mut ids = BTreeSet::new();
            for group in groups {
                ensure!(
                    !group.id.trim().is_empty() && !group.name.trim().is_empty(),
                    "identity id and name must not be blank"
                );
                ensure!(ids.insert(&group.id), "duplicate identity: {}", group.id);
                ensure!(
                    !group.records.is_empty(),
                    "identity {} must contain records",
                    group.id
                );
                assertion::validate_evidence(&group.evidence, &self.sources)
                    .with_context(|| format!("identity {}", group.id))?;
                let mut members = BTreeSet::new();
                for id in &group.records {
                    ensure!(
                        members.insert(id),
                        "identity {}: duplicate member {id}",
                        group.id
                    );
                    let record = records
                        .get(id.as_str())
                        .with_context(|| format!("identity {}: missing record {id}", group.id))?;
                    match kind {
                        ScopeKind::Model => {
                            ensure!(
                                record.kind == RecordKind::Model,
                                "identity {}: model membership requires a model record: {id}",
                                group.id
                            );
                            ensure!(
                                model_members.insert(id),
                                "record {id} belongs to multiple model identities"
                            );
                        }
                        ScopeKind::Interface => ensure!(
                            record.kind == RecordKind::Service,
                            "identity {}: interface binding requires a service record: {id}",
                            group.id
                        ),
                    }
                }
            }
        }
        Ok(())
    }

    /// Only explicit membership selects rules; references and hosting links do not copy rules.
    pub fn scopes_for_record(&self, record: &str) -> Vec<Scope> {
        let mut scopes = Vec::new();
        for (kind, groups) in [
            (ScopeKind::Model, &self.models),
            (ScopeKind::Interface, &self.interfaces),
        ] {
            let mut matching: Vec<_> = groups
                .iter()
                .filter(|group| group.records.iter().any(|id| id == record))
                .collect();
            matching.sort_by(|a, b| a.id.cmp(&b.id));
            scopes.extend(matching.into_iter().map(|group| Scope {
                kind,
                id: group.id.clone(),
            }));
        }
        scopes
    }

    pub fn model_for_record(&self, record: &str) -> Option<&Identity> {
        self.models
            .iter()
            .find(|group| group.records.iter().any(|id| id == record))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> (Registry, IdentitySet) {
        let records = [
            ("a:model:m", RecordKind::Model),
            ("b:model:m", RecordKind::Model),
            ("a:service:m", RecordKind::Service),
            ("host:service:m", RecordKind::Service),
        ]
        .into_iter()
        .map(|(id, kind)| {
            serde_json::from_value::<crate::model::Record>(json!({
                "schema_version":1,"kind":kind,"id":id,"source":id.split(':').next().unwrap(),
                "upstream_id":"m","name":"M","origin":{"snapshot":"fixture","pointer":""},"facts":{}
            }))
            .unwrap()
        })
        .collect::<Vec<_>>();
        let registry = Registry {
            catalogs: vec![
                serde_json::from_value(
                    json!({"schema_version":1,"source":"fixture","records":records,"snapshots":[]}),
                )
                .unwrap(),
            ],
            assertions: vec![],
        };
        let evidence = json!([{"source":"official","locator":"Exact release and API reference","checked_on":"2026-10-16","kind":"reported"}]);
        let identities = serde_json::from_value(json!({
            "schema_version":1,
            "sources":{"official":{"url":"https://example.com/release","checked_on":"2026-10-16"}},
            "models":[{"id":"publisher/m","name":"M","records":["a:model:m","b:model:m"],"evidence":evidence}],
            "interfaces":[{"id":"publisher/api","name":"Native API","records":["a:service:m"],"evidence":evidence}]
        })).unwrap();
        (registry, identities)
    }

    #[test]
    fn explicit_bindings_do_not_spread_interface_rules_across_models_or_hosts() {
        let (registry, identities) = fixture();
        identities.validate(&registry).unwrap();
        assert_eq!(
            identities.scopes_for_record("a:model:m"),
            vec![Scope {
                kind: ScopeKind::Model,
                id: "publisher/m".into()
            }]
        );
        assert_eq!(
            identities.scopes_for_record("a:service:m"),
            vec![Scope {
                kind: ScopeKind::Interface,
                id: "publisher/api".into()
            }]
        );
        assert!(identities.scopes_for_record("host:service:m").is_empty());
        assert_eq!(
            identities.model_for_record("b:model:m").unwrap().id,
            "publisher/m"
        );
    }

    #[test]
    fn rejects_ambiguous_identity_dangling_members_and_unsupported_evidence() {
        let (registry, original) = fixture();
        let mut identities = original.clone();
        let mut duplicate = identities.models[0].clone();
        duplicate.id = "publisher/other-release".into();
        identities.models.push(duplicate);
        assert!(identities.validate(&registry).is_err());
        for member in ["missing:model:m", "a:service:m"] {
            let mut identities = original.clone();
            identities.models[0].records = vec![member.into()];
            assert!(identities.validate(&registry).is_err());
        }
        let mut identities = original.clone();
        identities.interfaces[0].records = vec!["a:model:m".into()];
        assert!(identities.validate(&registry).is_err());
        let mut identities = original;
        identities.models[0].evidence[0].source = "unregistered".into();
        assert!(identities.validate(&registry).is_err());
    }
}
