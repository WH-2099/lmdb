use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::assertion::{
    self, Assertion, Condition, DocumentSource, Operator, Review, Scope, ScopeKind, Status,
};
use crate::registry::Registry;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MatchState {
    Matched,
    NotMatched,
    Undetermined,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConditionMatch {
    pub state: MatchState,
    pub missing_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuleCandidate {
    pub assertion: Assertion,
    pub condition_match: ConditionMatch,
    pub sources: BTreeMap<String, DocumentSource>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuleGroup {
    pub scope: Scope,
    pub property: String,
    pub candidates: Vec<RuleCandidate>,
    pub resolution: RuleResolution,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum RuleResolution {
    Known {
        status: Status,
        value: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unit: Option<String>,
    },
    Conflicted,
    Unknown,
    NoApplicableRule,
}

/// Evaluate validated conditions against literal path keys, without coercing JSON values.
pub fn evaluate(conditions: &[Condition], context: &BTreeMap<String, Value>) -> ConditionMatch {
    let mut missing_paths = BTreeSet::new();
    let mut not_matched = false;
    for condition in conditions {
        let Some(actual) = context.get(&condition.path) else {
            missing_paths.insert(condition.path.clone());
            continue;
        };
        let matches = match condition.op {
            Operator::Eq => actual == &condition.value,
            Operator::In => condition
                .value
                .as_array()
                .is_some_and(|values| values.contains(actual)),
        };
        not_matched |= !matches;
    }
    ConditionMatch {
        state: if not_matched {
            MatchState::NotMatched
        } else if missing_paths.is_empty() {
            MatchState::Matched
        } else {
            MatchState::Undetermined
        },
        missing_paths: missing_paths.into_iter().collect(),
    }
}

/// Empty scopes select all scopes; callers must preserve each result's scope when presenting it.
pub fn query(
    registry: &Registry,
    scopes: &[Scope],
    property: Option<&str>,
    context: &BTreeMap<String, Value>,
) -> Result<Vec<RuleGroup>> {
    let mut groups = BTreeMap::new();
    for set in &registry.assertions {
        assertion::validate(set)?;
        for assertion in &set.assertions {
            if (!scopes.is_empty() && !scopes.contains(&assertion.scope))
                || property.is_some_and(|property| assertion.property != property)
            {
                continue;
            }
            let kind = match assertion.scope.kind {
                ScopeKind::Model => "model",
                ScopeKind::Interface => "interface",
            };
            let group = groups
                .entry((
                    kind,
                    assertion.scope.id.as_str(),
                    assertion.property.as_str(),
                ))
                .or_insert_with(|| RuleGroup {
                    scope: assertion.scope.clone(),
                    property: assertion.property.clone(),
                    candidates: Vec::new(),
                    resolution: RuleResolution::Unknown,
                });
            group.candidates.push(RuleCandidate {
                assertion: assertion.clone(),
                condition_match: evaluate(&assertion.conditions, context),
                sources: assertion
                    .evidence
                    .iter()
                    .map(|evidence| {
                        (
                            evidence.source.clone(),
                            set.sources[&evidence.source].clone(),
                        )
                    })
                    .collect(),
            });
        }
    }
    Ok(groups
        .into_values()
        .map(|mut group| {
            group
                .candidates
                .sort_by(|a, b| a.assertion.id.cmp(&b.assertion.id));
            group.resolution = resolve(&group.candidates);
            group
        })
        .collect())
}

fn resolve(candidates: &[RuleCandidate]) -> RuleResolution {
    let matched: Vec<_> = candidates
        .iter()
        .filter(|candidate| candidate.condition_match.state == MatchState::Matched)
        .map(|candidate| &candidate.assertion)
        .collect();
    let known: Vec<_> = matched
        .iter()
        .filter(|assertion| assertion.status != Status::Unknown)
        .collect();
    if matched
        .iter()
        .any(|assertion| assertion.review == Review::Conflicted)
        || known.first().is_some_and(|first| {
            known.iter().any(|assertion| {
                assertion.status != first.status
                    || assertion.value != first.value
                    || assertion.unit != first.unit
            })
        })
    {
        return RuleResolution::Conflicted;
    }
    if matched
        .iter()
        .any(|assertion| assertion.status == Status::Unknown)
        || candidates
            .iter()
            .any(|candidate| candidate.condition_match.state == MatchState::Undetermined)
    {
        return RuleResolution::Unknown;
    }
    match matched.first() {
        None => RuleResolution::NoApplicableRule,
        Some(assertion) => RuleResolution::Known {
            status: assertion.status,
            value: assertion.value.clone(),
            unit: assertion.unit.clone(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assertion::{AssertionSet, Evidence, EvidenceKind};
    use serde_json::json;

    fn condition(path: &str, op: Operator, value: Value) -> Condition {
        Condition {
            path: path.into(),
            op,
            value,
        }
    }

    fn rule(id: &str, value: Value) -> Assertion {
        Assertion {
            id: id.into(),
            scope: Scope {
                kind: ScopeKind::Model,
                id: "example/model".into(),
            },
            property: "thinking.disable".into(),
            value,
            status: Status::Supported,
            conditions: Vec::new(),
            review: Review::Accepted,
            evidence: vec![Evidence {
                source: "official".into(),
                locator: "Thinking configuration".into(),
                checked_on: "2026-10-15".into(),
                kind: EvidenceKind::Reported,
            }],
            unit: None,
            notes: None,
        }
    }

    fn registry(assertions: Vec<Assertion>) -> Registry {
        Registry {
            catalogs: Vec::new(),
            assertions: vec![AssertionSet {
                schema_version: 1,
                sources: BTreeMap::from([(
                    "official".into(),
                    DocumentSource {
                        url: "https://example.com/models".into(),
                        title: None,
                        checked_on: "2026-10-15".into(),
                    },
                )]),
                assertions,
            }],
        }
    }

    fn resolution(assertions: Vec<Assertion>) -> RuleResolution {
        query(&registry(assertions), &[], None, &BTreeMap::new())
            .unwrap()
            .remove(0)
            .resolution
    }

    #[test]
    fn distinguishes_missing_values_from_typed_values_and_null() {
        let conditions = [condition("thinkingBudget", Operator::Eq, json!(0))];
        assert_eq!(
            evaluate(&conditions, &BTreeMap::new()),
            ConditionMatch {
                state: MatchState::Undetermined,
                missing_paths: vec!["thinkingBudget".into()],
            }
        );
        for (value, state) in [
            (json!(0), MatchState::Matched),
            (json!("0"), MatchState::NotMatched),
            (json!(false), MatchState::NotMatched),
            (Value::Null, MatchState::NotMatched),
        ] {
            let result = evaluate(
                &conditions,
                &BTreeMap::from([("thinkingBudget".into(), value)]),
            );
            assert_eq!(result.state, state);
            assert!(result.missing_paths.is_empty());
        }
        assert_eq!(
            evaluate(
                &[condition("optional", Operator::Eq, Value::Null)],
                &BTreeMap::from([("optional".into(), Value::Null)]),
            )
            .state,
            MatchState::Matched
        );
    }

    #[test]
    fn evaluates_membership_and_empty_conditions() {
        let conditions = [condition(
            "task_head",
            Operator::In,
            json!(["token_classification", "sequence_classification"]),
        )];
        for (value, state) in [
            (json!("sequence_classification"), MatchState::Matched),
            (json!("masked_language_model"), MatchState::NotMatched),
            (json!(["sequence_classification"]), MatchState::NotMatched),
        ] {
            assert_eq!(
                evaluate(&conditions, &BTreeMap::from([("task_head".into(), value)])).state,
                state
            );
        }
        assert_eq!(evaluate(&[], &BTreeMap::new()).state, MatchState::Matched);
    }

    #[test]
    fn false_condition_wins_over_missing_context_in_any_order() {
        let mut conditions = vec![
            condition("z.missing", Operator::Eq, json!(true)),
            condition("interface", Operator::Eq, json!("native")),
            condition("a.missing", Operator::Eq, json!(true)),
            condition("z.missing", Operator::Eq, json!(true)),
        ];
        let context = BTreeMap::from([("interface".into(), json!("other"))]);
        let expected = ConditionMatch {
            state: MatchState::NotMatched,
            missing_paths: vec!["a.missing".into(), "z.missing".into()],
        };
        assert_eq!(evaluate(&conditions, &context), expected);
        conditions.reverse();
        assert_eq!(evaluate(&conditions, &context), expected);
    }

    #[test]
    fn missing_context_or_inapplicable_rule_does_not_mean_unsupported() {
        let mut assertion = rule("conditional", json!(false));
        assertion.status = Status::Unsupported;
        assertion.conditions = vec![condition("interface", Operator::Eq, json!("native"))];
        let registry = registry(vec![assertion]);
        for (context, expected) in [
            (BTreeMap::new(), RuleResolution::Unknown),
            (
                BTreeMap::from([("interface".into(), json!("other"))]),
                RuleResolution::NoApplicableRule,
            ),
            (
                BTreeMap::from([("interface".into(), json!("native"))]),
                RuleResolution::Known {
                    status: Status::Unsupported,
                    value: json!(false),
                    unit: None,
                },
            ),
        ] {
            assert_eq!(
                query(&registry, &[], None, &context).unwrap()[0].resolution,
                expected
            );
        }
    }

    #[test]
    fn matched_conflicts_survive_missing_context_and_preserve_evidence() {
        let mut conflict = rule("conflict", Value::Null);
        conflict.status = Status::Unknown;
        conflict.review = Review::Conflicted;
        let mut pending = rule("pending", json!(true));
        pending.conditions = vec![condition("interface", Operator::Eq, json!("native"))];
        let registry = registry(vec![pending, conflict.clone()]);
        let groups = query(&registry, &[], None, &BTreeMap::new()).unwrap();
        assert_eq!(groups[0].resolution, RuleResolution::Conflicted);
        assert_eq!(groups[0].candidates[0].assertion, conflict);
        assert_eq!(
            groups[0].candidates[0].sources["official"].url,
            "https://example.com/models"
        );
    }

    #[test]
    fn conflict_requires_its_own_conditions_to_match() {
        let mut conflict = rule("conflict", Value::Null);
        conflict.status = Status::Unknown;
        conflict.review = Review::Conflicted;
        conflict.conditions = vec![condition("interface", Operator::Eq, json!("native"))];
        let registry = registry(vec![conflict, rule("known", json!(true))]);
        for (context, expected) in [
            (BTreeMap::new(), RuleResolution::Unknown),
            (
                BTreeMap::from([("interface".into(), json!("other"))]),
                RuleResolution::Known {
                    status: Status::Supported,
                    value: json!(true),
                    unit: None,
                },
            ),
            (
                BTreeMap::from([("interface".into(), json!("native"))]),
                RuleResolution::Conflicted,
            ),
        ] {
            assert_eq!(
                query(&registry, &[], None, &context).unwrap()[0].resolution,
                expected
            );
        }
    }

    #[test]
    fn differing_results_conflict_and_agreement_keeps_typed_value_and_unit() {
        let mut first = rule("first", json!([256, 512]));
        first.unit = Some("dimensions".into());
        let mut second = first.clone();
        second.id = "second".into();
        assert_eq!(
            resolution(vec![first.clone(), second.clone()]),
            RuleResolution::Known {
                status: Status::Supported,
                value: json!([256, 512]),
                unit: Some("dimensions".into()),
            }
        );
        for changed in [
            Assertion {
                value: json!([512]),
                ..second.clone()
            },
            Assertion {
                status: Status::Unsupported,
                ..second.clone()
            },
            Assertion {
                unit: None,
                ..second
            },
        ] {
            assert_eq!(
                resolution(vec![first.clone(), changed]),
                RuleResolution::Conflicted
            );
        }
    }

    #[test]
    fn unresolved_candidates_and_accepted_unknown_remain_unknown() {
        let known = rule("known", json!(true));
        let mut pending = rule("pending", json!(true));
        pending.conditions = vec![condition("interface", Operator::Eq, json!("native"))];
        assert_eq!(resolution(vec![known, pending]), RuleResolution::Unknown);
        let mut unknown = rule("unknown", Value::Null);
        unknown.status = Status::Unknown;
        assert_eq!(resolution(vec![unknown.clone()]), RuleResolution::Unknown);
        assert_eq!(
            resolution(vec![unknown, rule("known", json!(true))]),
            RuleResolution::Unknown
        );
    }

    #[test]
    fn keeps_scope_kinds_separate_and_filters_exact_property() {
        let mut model = rule("z-model", json!(true));
        model.property = "embedding.output_dimensions".into();
        let mut interface = model.clone();
        interface.id = "a-interface".into();
        interface.scope.kind = ScopeKind::Interface;
        interface.value = json!(false);
        let registry = registry(vec![model.clone(), interface]);
        let groups = query(&registry, &[], None, &BTreeMap::new()).unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].scope.kind, ScopeKind::Interface);
        assert_eq!(groups[1].scope.kind, ScopeKind::Model);
        let selected = query(
            &registry,
            &[model.scope],
            Some("embedding.output_dimensions"),
            &BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].candidates[0].assertion.id, "z-model");
        assert!(
            query(&registry, &[], Some("embedding"), &BTreeMap::new())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn rejects_invalid_assertions_before_evaluating_them() {
        let mut invalid = rule("invalid", json!(true));
        invalid.conditions = vec![condition("task", Operator::In, json!([]))];
        assert!(query(&registry(vec![invalid]), &[], None, &BTreeMap::new()).is_err());
    }

    #[test]
    fn source_keys_are_local_and_results_ignore_file_order() {
        let mut registry = registry(vec![rule("first", json!(true))]);
        let mut second = registry.assertions[0].clone();
        second.assertions[0].id = "second".into();
        second.sources.get_mut("official").unwrap().url =
            "https://example.com/other-document".into();
        registry.assertions.push(second);
        let result = query(&registry, &[], None, &BTreeMap::new()).unwrap();
        assert_eq!(
            result[0].candidates[0].sources["official"].url,
            "https://example.com/models"
        );
        assert_eq!(
            result[0].candidates[1].sources["official"].url,
            "https://example.com/other-document"
        );
        registry.assertions.reverse();
        assert_eq!(
            query(&registry, &[], None, &BTreeMap::new()).unwrap(),
            result
        );
    }
}
