use crate::model::Snapshot;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Html,
    Markdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DocumentKind {
    ReleaseBlog,
    ModelCard,
    ModelPage,
    Documentation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PageSource {
    pub id: String,
    pub vendor: String,
    pub kind: DocumentKind,
    pub url: String,
    pub format: Format,
    pub records: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PageConfig {
    pub schema_version: u32,
    pub pages: Vec<PageSource>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PageEvidence {
    /// CSS path in original HTML, or rendered Markdown DOM, or explicit Markdown byte range.
    pub locator: String,
    /// Short whitespace-normalized DOM text or attribute, or a literal Markdown fragment.
    /// The complete response and deterministic extraction preserve the original evidence.
    pub excerpt: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Precision {
    Exact,
    Approximate,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClaimScope {
    Model,
    Interface {
        id: String,
    },
    #[default]
    Unspecified,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Claim {
    pub record: String,
    pub property: String,
    pub value: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    pub precision: Precision,
    #[serde(default)]
    pub scope: ClaimScope,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<String>,
    pub evidence: PageEvidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PassageTopic {
    Overview,
    Capabilities,
    Limitations,
    Availability,
    Training,
    Evaluation,
    Usage,
    License,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Passage {
    pub topic: PassageTopic,
    pub heading: Option<String>,
    /// Explicit subjects; empty means the passage concerns the page or model family.
    pub records: Vec<String>,
    /// Published DOM text with original wording and whitespace, excluding outer whitespace.
    #[serde(serialize_with = "serialize_original_text")]
    #[schemars(with = "String")]
    pub text: String,
    pub evidence: PageEvidence,
}

fn serialize_original_text<S: serde::Serializer>(
    text: &str,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    // Escaped newlines preserve source whitespace without whitespace-only YAML lines.
    if text.contains('\n') {
        serde_saphyr::DoubleQuoted(text).serialize(serializer)
    } else {
        text.serialize(serializer)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LocatedValue {
    pub value: String,
    pub evidence: PageEvidence,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OfficialLink {
    pub url: String,
    pub label: String,
    pub evidence: PageEvidence,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Extraction {
    pub extractor_version: u32,
    pub title: Option<LocatedValue>,
    pub published_at: Option<LocatedValue>,
    pub canonical_url: Option<LocatedValue>,
    pub links: Vec<OfficialLink>,
    #[serde(default)]
    pub passages: Vec<Passage>,
    pub claims: Vec<Claim>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PageContent {
    pub snapshot: Snapshot,
    pub content_type: Option<String>,
    pub extraction: Extraction,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PageDocument {
    pub source: PageSource,
    pub checked_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<PageContent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocumentCatalog {
    pub schema_version: u32,
    pub documents: Vec<PageDocument>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_passage_whitespace_survives_yaml_and_json() {
        let passage = Passage {
            topic: PassageTopic::Overview,
            heading: None,
            records: vec![],
            text: "First paragraph.\n \nSecond\tparagraph.".into(),
            evidence: PageEvidence {
                locator: "css:p".into(),
                excerpt: "First paragraph. Second paragraph.".into(),
            },
        };
        let yaml = serde_saphyr::to_string(&passage).unwrap();
        assert!(yaml.lines().all(|line| line == line.trim_end()));
        assert_eq!(serde_saphyr::from_str::<Passage>(&yaml).unwrap(), passage);
        assert_eq!(
            serde_json::to_value(&passage).unwrap()["text"],
            passage.text
        );
    }
}
