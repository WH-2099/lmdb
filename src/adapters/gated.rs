use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail, ensure};
use reqwest::Url;
use serde_json::Value;

use crate::model::{Fact, Origin, Record, RecordKind, SCHEMA_VERSION, Snapshot};

pub struct RequestSpec {
    pub url: &'static str,
    pub credential_env: &'static str,
    pub header: &'static str,
    pub prefix: &'static str,
    pub extra_headers: Vec<(&'static str, &'static str)>,
}

/// Describes a request without reading credentials or making network calls.
pub fn request_spec(source: &str) -> Option<RequestSpec> {
    let (url, credential_env, header, prefix) = match source {
        "artificialanalysis" => (
            "https://artificialanalysis.ai/api/v2/language/models/free",
            "ARTIFICIAL_ANALYSIS_API_KEY",
            "x-api-key",
            "",
        ),
        "openai" => (
            "https://api.openai.com/v1/models",
            "OPENAI_API_KEY",
            "authorization",
            "Bearer ",
        ),
        "anthropic" => (
            "https://api.anthropic.com/v1/models",
            "ANTHROPIC_API_KEY",
            "x-api-key",
            "",
        ),
        "gemini" => (
            "https://generativelanguage.googleapis.com/v1beta/models",
            "GEMINI_API_KEY",
            "x-goog-api-key",
            "",
        ),
        "deepseek" => (
            "https://api.deepseek.com/models",
            "DEEPSEEK_API_KEY",
            "authorization",
            "Bearer ",
        ),
        "cohere" => (
            "https://api.cohere.com/v1/models",
            "COHERE_API_KEY",
            "authorization",
            "Bearer ",
        ),
        "fal" => (
            "https://api.fal.ai/v1/models",
            "FAL_KEY",
            "authorization",
            "Key ",
        ),
        "replicate" => (
            "https://api.replicate.com/v1/models",
            "REPLICATE_API_TOKEN",
            "authorization",
            "Bearer ",
        ),
        _ => return None,
    };
    Some(RequestSpec {
        url,
        credential_env,
        header,
        prefix,
        extra_headers: if source == "anthropic" {
            vec![("anthropic-version", "2023-06-01")]
        } else {
            Vec::new()
        },
    })
}

pub fn normalize(source: &str, value: &Value, snapshot: &Snapshot) -> Result<Vec<Record>> {
    check_response(source, value)?;
    ensure!(
        snapshot.source == source,
        "snapshot source does not match {source}"
    );
    let (array_pointer, id_pointer, name_pointer, fields): (_, _, _, &[(&str, &str)]) = match source
    {
        "artificialanalysis" => (
            "/data",
            "/id",
            "/name",
            &[
                ("slug", "/slug"),
                ("release_date", "/release_date"),
                ("model_creator", "/model_creator"),
                ("pricing", "/pricing"),
                ("performance", "/performance"),
            ],
        ),
        "openai" => (
            "/data",
            "/id",
            "/id",
            &[
                ("owned_by", "/owned_by"),
                ("created", "/created"),
                ("shutdown_date", "/shutdown_date"),
            ],
        ),
        "anthropic" => (
            "/data",
            "/id",
            "/display_name",
            &[
                ("created_at", "/created_at"),
                ("capabilities", "/capabilities"),
                ("max_input_tokens", "/max_input_tokens"),
                ("max_output_tokens", "/max_tokens"),
                ("lifecycle", "/lifecycle"),
                ("line", "/line"),
                ("deprecated_at", "/deprecated_at"),
                ("retires_at", "/retires_at"),
            ],
        ),
        "gemini" => (
            "/models",
            "/name",
            "/displayName",
            &[
                ("base_model_id", "/baseModelId"),
                ("version", "/version"),
                ("description", "/description"),
                ("max_input_tokens", "/inputTokenLimit"),
                ("max_output_tokens", "/outputTokenLimit"),
                (
                    "supported_generation_methods",
                    "/supportedGenerationMethods",
                ),
                ("thinking", "/thinking"),
                ("temperature", "/temperature"),
                ("max_temperature", "/maxTemperature"),
                ("top_p", "/topP"),
                ("top_k", "/topK"),
            ],
        ),
        "deepseek" => (
            "/data",
            "/id",
            "/name",
            &[
                ("owned_by", "/owned_by"),
                ("context_window", "/context_window"),
                ("max_output_tokens", "/max_output_tokens"),
                ("input_modalities", "/input_modalities"),
                ("output_modalities", "/output_modalities"),
                ("effort", "/effort"),
                ("api_capabilities", "/api_capabilities"),
            ],
        ),
        "cohere" => (
            "/models",
            "/name",
            "/name",
            &[
                ("is_deprecated", "/is_deprecated"),
                ("endpoints", "/endpoints"),
                ("finetuned", "/finetuned"),
                ("context_window", "/context_length"),
                ("tokenizer_url", "/tokenizer_url"),
                ("default_endpoints", "/default_endpoints"),
                ("features", "/features"),
                ("sampling_defaults", "/sampling_defaults"),
            ],
        ),
        "fal" => (
            "/models",
            "/endpoint_id",
            "/metadata/display_name",
            &[
                ("category", "/metadata/category"),
                ("description", "/metadata/description"),
                ("status", "/metadata/status"),
                ("tags", "/metadata/tags"),
                ("updated_at", "/metadata/updated_at"),
                ("model_url", "/metadata/model_url"),
                ("published_at", "/metadata/date"),
            ],
        ),
        "replicate" => (
            "/results",
            "/name",
            "/name",
            &[
                ("owner", "/owner"),
                ("description", "/description"),
                ("url", "/url"),
                ("visibility", "/visibility"),
                ("github_url", "/github_url"),
                ("paper_url", "/paper_url"),
                ("license_url", "/license_url"),
                ("weights_url", "/weights_url"),
                ("latest_version_id", "/latest_version/id"),
                ("latest_version_created_at", "/latest_version/created_at"),
            ],
        ),
        _ => unreachable!("check_response validates source"),
    };
    let items = value
        .pointer(array_pointer)
        .and_then(Value::as_array)
        .with_context(|| format!("{source}: expected array at {array_pointer}"))?;
    ensure!(!items.is_empty(), "{source}: empty catalog response");
    let mut ids = BTreeSet::new();
    items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let pointer = format!("{array_pointer}/{index}");
            let id = required_string(item.pointer(id_pointer), &format!("{pointer}{id_pointer}"))?;
            let upstream_id = if source == "replicate" {
                let owner = required_string(item.get("owner"), &format!("{pointer}/owner"))?;
                format!("{owner}/{id}")
            } else {
                id.to_owned()
            };
            ensure!(
                ids.insert(upstream_id.clone()),
                "{source}: duplicate id {upstream_id}"
            );
            let name = match item.pointer(name_pointer) {
                None | Some(Value::Null) => id,
                value => required_string(value, &format!("{pointer}{name_pointer}"))?,
            };
            let mut facts = BTreeMap::new();
            for &(name, field_pointer) in fields {
                if let Some(field) = item.pointer(field_pointer).filter(|field| !field.is_null()) {
                    facts.insert(
                        name.to_owned(),
                        Fact {
                            value: field.clone(),
                            pointer: format!("{pointer}{field_pointer}"),
                            unit: matches!(
                                name,
                                "context_window" | "max_input_tokens" | "max_output_tokens"
                            )
                            .then(|| "tokens".into()),
                        },
                    );
                }
            }
            Ok(Record {
                schema_version: SCHEMA_VERSION,
                kind: RecordKind::Service,
                id: format!("{source}:service:{upstream_id}"),
                source: source.to_owned(),
                upstream_id,
                name: name.to_owned(),
                model: None,
                origin: Origin {
                    snapshot: snapshot.id.clone(),
                    pointer,
                },
                facts,
            })
        })
        .collect()
}

/// Produces the next URL; the caller must enforce origin and credential policy.
pub fn next_page(source: &str, value: &Value, current_url: &str) -> Result<Option<String>> {
    check_response(source, value)?;
    match source {
        "openai" | "deepseek" => Ok(None),
        "artificialanalysis" => {
            let pagination = value
                .get("pagination")
                .and_then(Value::as_object)
                .context("artificialanalysis: missing pagination object")?;
            let page = pagination
                .get("page")
                .and_then(Value::as_u64)
                .filter(|page| *page > 0)
                .context("artificialanalysis: invalid page")?;
            let more = pagination
                .get("has_more")
                .and_then(Value::as_bool)
                .context("artificialanalysis: invalid has_more")?;
            if let Some(total) = pagination.get("total_pages") {
                let total = total
                    .as_u64()
                    .filter(|total| *total >= page)
                    .context("artificialanalysis: invalid total_pages")?;
                ensure!(
                    more == (page < total),
                    "artificialanalysis: inconsistent pagination"
                );
            }
            if more {
                let next = page
                    .checked_add(1)
                    .context("artificialanalysis: page overflow")?;
                Ok(Some(with_query(current_url, "page", &next.to_string())?))
            } else {
                Ok(None)
            }
        }
        "anthropic" | "fal" => {
            let more = value
                .get("has_more")
                .and_then(Value::as_bool)
                .with_context(|| format!("{source}: invalid has_more"))?;
            let (field, parameter) = if source == "anthropic" {
                ("last_id", "after_id")
            } else {
                ("next_cursor", "cursor")
            };
            let cursor = optional_cursor(value.get(field), field)?;
            if more {
                let cursor =
                    cursor.with_context(|| format!("{source}: has_more without {field}"))?;
                Ok(Some(with_query(current_url, parameter, cursor)?))
            } else {
                ensure!(
                    source != "fal" || cursor.is_none(),
                    "fal: cursor present with has_more=false"
                );
                Ok(None)
            }
        }
        "gemini" | "cohere" => {
            let (field, parameter) = if source == "gemini" {
                ("nextPageToken", "pageToken")
            } else {
                ("next_page_token", "page_token")
            };
            optional_cursor(value.get(field), field)?
                .map(|cursor| with_query(current_url, parameter, cursor))
                .transpose()
        }
        "replicate" => {
            let next = value
                .get("next")
                .context("replicate: missing next pagination field")?;
            optional_cursor(Some(next), "next")?
                .map(|next| {
                    let url = Url::parse(next).context("replicate: invalid next URL")?;
                    ensure!(
                        url.scheme() == "https" && url.has_host(),
                        "replicate: next URL must use HTTPS"
                    );
                    ensure!(
                        url.as_str() != current_url,
                        "replicate: pagination did not advance"
                    );
                    Ok(url.into())
                })
                .transpose()
        }
        _ => unreachable!("check_response validates source"),
    }
}

fn check_response(source: &str, value: &Value) -> Result<()> {
    ensure!(
        request_spec(source).is_some(),
        "unknown authenticated source: {source}"
    );
    ensure!(value.is_object(), "{source}: expected response object");
    if value.get("error").is_some_and(|error| !error.is_null()) {
        bail!("{source}: upstream error response");
    }
    Ok(())
}

fn required_string<'a>(value: Option<&'a Value>, path: &str) -> Result<&'a str> {
    value
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .with_context(|| format!("expected nonempty string at {path}"))
}

fn optional_cursor<'a>(value: Option<&'a Value>, path: &str) -> Result<Option<&'a str>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        value => required_string(value, path).map(Some),
    }
}

fn with_query(current_url: &str, name: &str, value: &str) -> Result<String> {
    let mut url = Url::parse(current_url).context("invalid current page URL")?;
    let parameters: Vec<_> = url
        .query_pairs()
        .filter(|(key, _)| key != name)
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    url.set_query(None);
    url.query_pairs_mut()
        .extend_pairs(parameters)
        .append_pair(name, value);
    ensure!(url.as_str() != current_url, "pagination did not advance");
    Ok(url.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{model::Catalog, registry::Registry};
    use serde_json::json;

    const FIXTURES: &[(&str, &str)] = &[
        (
            "artificialanalysis",
            include_str!("../../tests/fixtures/gated/artificialanalysis.json"),
        ),
        (
            "openai",
            include_str!("../../tests/fixtures/gated/openai.json"),
        ),
        (
            "anthropic",
            include_str!("../../tests/fixtures/gated/anthropic.json"),
        ),
        (
            "gemini",
            include_str!("../../tests/fixtures/gated/gemini.json"),
        ),
        (
            "deepseek",
            include_str!("../../tests/fixtures/gated/deepseek.json"),
        ),
        (
            "cohere",
            include_str!("../../tests/fixtures/gated/cohere.json"),
        ),
        ("fal", include_str!("../../tests/fixtures/gated/fal.json")),
        (
            "replicate",
            include_str!("../../tests/fixtures/gated/replicate.json"),
        ),
    ];

    fn snapshot(source: &str) -> Snapshot {
        Snapshot {
            schema_version: SCHEMA_VERSION,
            id: format!("{source}:{}", "0".repeat(64)),
            source: source.into(),
            url: request_spec(source).unwrap().url.into(),
            retrieved_at: "2026-01-01T00:00:00Z".into(),
            sha256: "0".repeat(64),
            upstream_revision: None,
        }
    }

    #[test]
    fn fixtures_preserve_evidence_and_service_scope() {
        for &(source, fixture) in FIXTURES {
            let value: Value = serde_json::from_str(fixture).unwrap();
            let snapshot = snapshot(source);
            let records = normalize(source, &value, &snapshot).unwrap();
            assert_eq!(records.len(), 1, "{source}");
            let record = &records[0];
            assert_eq!(record.kind, RecordKind::Service);
            assert!(record.id.starts_with(&format!("{source}:service:")));
            assert!(record.model.is_none());
            assert!(value.pointer(&record.origin.pointer).is_some());
            assert!(!record.facts.is_empty());
            for fact in record.facts.values() {
                assert_eq!(value.pointer(&fact.pointer), Some(&fact.value));
            }
            assert_eq!(next_page(source, &value, &snapshot.url).unwrap(), None);
            Registry {
                catalogs: vec![Catalog {
                    schema_version: SCHEMA_VERSION,
                    source: source.into(),
                    last_sync: None,
                    snapshots: vec![snapshot],
                    records,
                }],
                assertions: Vec::new(),
            }
            .validate(None)
            .unwrap();
        }
    }

    #[test]
    fn replicate_identity_includes_owner() {
        let value = json!({"results": [{"owner": "one", "name": "model"}, {"owner": "two", "name": "model"}]});
        let records = normalize("replicate", &value, &snapshot("replicate")).unwrap();
        assert_eq!(records[0].upstream_id, "one/model");
        assert_eq!(records[1].upstream_id, "two/model");
    }

    #[test]
    fn unknown_and_null_fields_are_omitted() {
        let value = json!({"data": [{"id": "test", "owned_by": null, "unknown": true}]});
        let records = normalize("openai", &value, &snapshot("openai")).unwrap();
        assert_eq!(records[0].name, "test");
        assert!(records[0].facts.is_empty());
    }

    #[test]
    fn invalid_envelopes_and_identities_fail() {
        for &(source, fixture) in FIXTURES {
            for value in [
                json!({}),
                json!([]),
                json!({"error": "denied"}),
                json!({"data": [], "models": [], "results": []}),
            ] {
                assert!(
                    normalize(source, &value, &snapshot(source)).is_err(),
                    "{source}"
                );
            }
            let mut value: Value = serde_json::from_str(fixture).unwrap();
            let key = match source {
                "gemini" | "cohere" | "fal" => "models",
                "replicate" => "results",
                _ => "data",
            };
            value[key] = json!([{}]);
            assert!(normalize(source, &value, &snapshot(source)).is_err());
        }
        assert!(
            normalize(
                "openai",
                &json!({"data": [{"id": "a"}, {"id": "a"}]}),
                &snapshot("openai")
            )
            .is_err()
        );
        assert!(
            normalize(
                "openai",
                &json!({"data": [{"id": " "}]}),
                &snapshot("openai")
            )
            .is_err()
        );
        assert!(
            normalize(
                "openai",
                &json!({"data": [{"id": "a"}]}),
                &snapshot("gemini")
            )
            .is_err()
        );
    }

    #[test]
    fn pagination_preserves_parameters_and_escapes_cursors() {
        for (source, body, parameter, expected) in [
            (
                "artificialanalysis",
                json!({"pagination": {"page": 1, "total_pages": 2, "has_more": true}}),
                "page",
                "2",
            ),
            (
                "anthropic",
                json!({"has_more": true, "last_id": "model/a+b"}),
                "after_id",
                "model/a+b",
            ),
            (
                "gemini",
                json!({"nextPageToken": "token/a+b"}),
                "pageToken",
                "token/a+b",
            ),
            (
                "cohere",
                json!({"next_page_token": "token/a+b"}),
                "page_token",
                "token/a+b",
            ),
            (
                "fal",
                json!({"has_more": true, "next_cursor": "token/a+b"}),
                "cursor",
                "token/a+b",
            ),
        ] {
            let url = format!(
                "{}?limit=20&{parameter}=old",
                request_spec(source).unwrap().url
            );
            let next = next_page(source, &body, &url).unwrap().unwrap();
            let next = Url::parse(&next).unwrap();
            let pairs: Vec<_> = next.query_pairs().collect();
            assert_eq!(pairs.len(), 2);
            assert!(
                pairs
                    .iter()
                    .any(|(key, value)| key == "limit" && value == "20")
            );
            assert!(
                pairs
                    .iter()
                    .any(|(key, value)| key == parameter && value == expected)
            );
        }
        let next = "https://api.replicate.com/v1/models?cursor=next";
        assert_eq!(
            next_page(
                "replicate",
                &json!({"next": next}),
                request_spec("replicate").unwrap().url
            )
            .unwrap()
            .as_deref(),
            Some(next)
        );
    }

    #[test]
    fn malformed_pagination_cannot_silently_end_a_sync() {
        for (source, body) in [
            ("artificialanalysis", json!({})),
            (
                "artificialanalysis",
                json!({"pagination": {"page": 0, "has_more": true}}),
            ),
            (
                "artificialanalysis",
                json!({"pagination": {"page": 1, "has_more": "true"}}),
            ),
            (
                "artificialanalysis",
                json!({"pagination": {"page": 1, "has_more": false, "total_pages": 2}}),
            ),
            ("anthropic", json!({"has_more": true})),
            ("anthropic", json!({"has_more": "true", "last_id": "a"})),
            ("gemini", json!({"nextPageToken": 1})),
            ("cohere", json!({"next_page_token": []})),
            ("fal", json!({"has_more": true, "next_cursor": null})),
            ("fal", json!({"has_more": false, "next_cursor": "a"})),
            ("replicate", json!({})),
            ("replicate", json!({"next": false})),
            ("replicate", json!({"next": "/v1/models?cursor=a"})),
            (
                "replicate",
                json!({"next": "http://api.replicate.com/v1/models?cursor=a"}),
            ),
        ] {
            assert!(
                next_page(source, &body, request_spec(source).unwrap().url).is_err(),
                "{source}: {body}"
            );
        }
    }

    #[test]
    fn repeated_cursor_fails() {
        let url = "https://api.cohere.com/v1/models?page_token=same";
        assert!(next_page("cohere", &json!({"next_page_token": "same"}), url).is_err());
        let url = request_spec("replicate").unwrap().url;
        assert!(next_page("replicate", &json!({"next": url}), url).is_err());
    }

    #[test]
    fn request_specs_have_no_secrets_or_query_keys() {
        for &(source, _) in FIXTURES {
            let spec = request_spec(source).unwrap();
            let url = Url::parse(spec.url).unwrap();
            assert_eq!(url.scheme(), "https");
            assert!(url.query().is_none());
            assert!(!spec.credential_env.is_empty());
        }
        assert!(request_spec("unknown").is_none());
        assert!(next_page("unknown", &json!({}), "https://example.com").is_err());
        let spec = request_spec("artificialanalysis").unwrap();
        assert!(spec.url.ends_with("/free"));
        assert_eq!(spec.header, "x-api-key");
        assert_eq!(
            request_spec("anthropic").unwrap().extra_headers,
            [("anthropic-version", "2023-06-01")]
        );
    }
}
