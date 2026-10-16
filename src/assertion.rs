use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AssertionSet {
    pub schema_version: u32,
    pub sources: BTreeMap<String, DocumentSource>,
    pub assertions: Vec<Assertion>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocumentSource {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub checked_on: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Assertion {
    pub id: String,
    pub scope: Scope,
    pub property: String,
    pub value: Value,
    pub status: Status,
    pub conditions: Vec<Condition>,
    pub review: Review,
    pub evidence: Vec<Evidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub kind: ScopeKind,
    pub id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScopeKind {
    Model,
    Interface,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Supported,
    Unsupported,
    Unknown,
    NotApplicable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Condition {
    pub path: String,
    pub op: Operator,
    pub value: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Operator {
    Eq,
    In,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Review {
    Accepted,
    Conflicted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub source: String,
    pub locator: String,
    pub checked_on: String,
    pub kind: EvidenceKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    Reported,
}

pub fn validate(set: &AssertionSet) -> Result<()> {
    ensure!(
        set.schema_version == 1,
        "unsupported assertion schema version"
    );
    validate_sources(&set.sources)?;

    let mut ids = BTreeSet::new();
    for assertion in &set.assertions {
        let id = &assertion.id;
        ensure!(!id.trim().is_empty(), "assertion id must not be blank");
        ensure!(ids.insert(id), "duplicate assertion id: {id}");
        ensure!(
            !assertion.scope.id.trim().is_empty(),
            "assertion {id}: scope id must not be blank"
        );
        ensure!(
            !assertion.property.trim().is_empty(),
            "assertion {id}: property must not be blank"
        );
        if let Some(unit) = &assertion.unit {
            ensure!(
                !unit.trim().is_empty(),
                "assertion {id}: unit must not be blank"
            );
            ensure!(
                !assertion.value.is_boolean(),
                "assertion {id}: boolean values cannot have a measurement unit"
            );
        }
        match assertion.status {
            Status::Unknown => ensure!(
                assertion.value.is_null(),
                "assertion {id}: unknown status requires a null value"
            ),
            Status::Supported | Status::Unsupported => ensure!(
                !assertion.value.is_null(),
                "assertion {id}: supported or unsupported status requires a value"
            ),
            Status::NotApplicable => {}
        }
        if assertion.review == Review::Conflicted {
            ensure!(
                assertion.status == Status::Unknown && assertion.value.is_null(),
                "assertion {id}: conflicted review requires unknown status and a null value"
            );
        }
        for condition in &assertion.conditions {
            ensure!(
                !condition.path.trim().is_empty(),
                "assertion {id}: condition path must not be blank"
            );
            if condition.op == Operator::In {
                ensure!(
                    condition
                        .value
                        .as_array()
                        .is_some_and(|values| !values.is_empty()),
                    "assertion {id}: in condition requires a nonempty array"
                );
            }
        }
        validate_evidence(&assertion.evidence, &set.sources)
            .with_context(|| format!("assertion {id}"))?;
    }
    Ok(())
}

pub fn validate_sources(sources: &BTreeMap<String, DocumentSource>) -> Result<()> {
    for (id, source) in sources {
        ensure!(!id.trim().is_empty(), "source id must not be blank");
        let url = reqwest::Url::parse(&source.url)
            .with_context(|| format!("source {id}: invalid URL"))?;
        ensure!(
            url.scheme() == "https" && url.host_str().is_some(),
            "source {id}: URL must use HTTPS and have a host"
        );
        validate_date(&source.checked_on)
            .with_context(|| format!("source {id}: invalid checked_on"))?;
    }
    Ok(())
}

pub fn validate_evidence(
    evidence: &[Evidence],
    sources: &BTreeMap<String, DocumentSource>,
) -> Result<()> {
    ensure!(!evidence.is_empty(), "evidence is required");
    for item in evidence {
        ensure!(
            sources.contains_key(&item.source),
            "unknown evidence source {}",
            item.source
        );
        ensure!(
            !item.locator.trim().is_empty(),
            "evidence locator must not be blank"
        );
        validate_date(&item.checked_on).context("invalid evidence checked_on")?;
    }
    Ok(())
}

fn validate_date(value: &str) -> Result<()> {
    ensure!(
        value.len() == 10
            && value.bytes().enumerate().all(|(i, b)| match i {
                4 | 7 => b == b'-',
                _ => b.is_ascii_digit(),
            }),
        "date must use YYYY-MM-DD"
    );
    time::Date::parse(
        value,
        &time::format_description::parse_borrowed::<2>("[year]-[month]-[day]")?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> AssertionSet {
        serde_json::from_value(json!({
            "schema_version": 1,
            "sources": {
                "official": {"url": "https://example.com/models", "checked_on": "2026-10-15"}
            },
            "assertions": [{
                "id": "dimensions",
                "scope": {"kind": "model", "id": "example/embed"},
                "property": "embedding.dimensions",
                "value": [256, 512],
                "status": "supported",
                "conditions": [],
                "review": "accepted",
                "evidence": [{
                    "source": "official", "locator": "Model dimensions", "checked_on": "2026-10-15", "kind": "reported"
                }],
                "unit": "dimensions"
            }]
        }))
        .unwrap()
    }

    #[test]
    fn accepts_typed_values_and_distinct_absence_states() {
        let mut set = fixture();
        validate(&set).unwrap();
        let assertion = &mut set.assertions[0];
        assertion.unit = None;
        assertion.status = Status::Unsupported;
        assertion.value = json!(false);
        validate(&set).unwrap();
        set.assertions[0].status = Status::Unknown;
        set.assertions[0].value = Value::Null;
        validate(&set).unwrap();
        set.assertions[0].status = Status::NotApplicable;
        validate(&set).unwrap();
        set.assertions[0].status = Status::Supported;
        set.assertions[0].value = json!([]);
        validate(&set).unwrap();
    }

    #[test]
    fn rejects_dangling_evidence() {
        let mut set = fixture();
        set.assertions[0].evidence[0].source = "missing".into();
        assert!(
            validate(&set)
                .unwrap_err()
                .root_cause()
                .to_string()
                .contains("unknown evidence source")
        );
    }

    #[test]
    fn rejects_conflict_with_false_value() {
        let mut set = fixture();
        set.assertions[0].unit = None;
        set.assertions[0].status = Status::Unsupported;
        set.assertions[0].value = json!(false);
        set.assertions[0].review = Review::Conflicted;
        assert!(
            validate(&set)
                .unwrap_err()
                .to_string()
                .contains("conflicted review")
        );
    }

    #[test]
    fn rejects_nonarray_or_empty_membership_conditions() {
        for value in [json!("AUDIO"), json!([])] {
            let mut set = fixture();
            set.assertions[0].conditions.push(Condition {
                path: "response_modality".into(),
                op: Operator::In,
                value,
            });
            assert!(
                validate(&set)
                    .unwrap_err()
                    .to_string()
                    .contains("nonempty array")
            );
        }
    }

    #[test]
    fn rejects_duplicate_ids_and_invalid_dates() {
        let mut set = fixture();
        set.assertions.push(set.assertions[0].clone());
        assert!(
            validate(&set)
                .unwrap_err()
                .to_string()
                .contains("duplicate assertion id")
        );
        for date in ["2026-02-29", "2026-2-01", "2026-01-01T00:00:00Z"] {
            assert!(validate_date(date).is_err());
        }
        validate_date("2024-02-29").unwrap();
    }
}
