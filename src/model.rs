use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RecordKind {
    Model,
    Service,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Fact {
    pub value: Value,
    /// JSON Pointer into the original response, including the record path.
    pub pointer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Origin {
    pub snapshot: String,
    pub pointer: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub schema_version: u32,
    pub kind: RecordKind,
    pub id: String,
    pub source: String,
    pub upstream_id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub origin: Origin,
    pub facts: BTreeMap<String, Fact>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub schema_version: u32,
    pub id: String,
    pub source: String,
    pub url: String,
    pub retrieved_at: String,
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_revision: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SyncManifest {
    pub completed_at: String,
    pub returned_ids: Vec<String>,
    pub missing_ids: Vec<String>,
    pub snapshot_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub schema_version: u32,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_sync: Option<SyncManifest>,
    pub snapshots: Vec<Snapshot>,
    pub records: Vec<Record>,
}

/// Escape one JSON Pointer segment (RFC 6901).
pub fn pointer_segment(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}
