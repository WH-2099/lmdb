use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result, ensure};
use reqwest::{Url, header::HeaderMap};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
    http::{Fetcher, checked_next_url},
    model::Snapshot,
    store, sync,
};

const MAX_RULE_RESULTS: usize = 20;
const MAX_TOTAL_RESULTS: usize = 200;
const PAGE_SIZE: usize = 10;
const DISCOVERY_SOURCES: &[&str] = &["huggingface", "modelscope"];

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceFile {
    pub schema_version: u32,
    pub sources: Vec<SourceDefinition>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceDefinition {
    pub id: String,
    pub access: Access,
    pub coverage: Coverage,
    pub auth_contract: AuthContract,
    pub terms_status: TermsStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_license: Option<String>,
    pub terms_urls: Vec<String>,
    pub docs_urls: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    Public,
    AuthenticatedOptIn,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Coverage {
    pub scope: CoverageScope,
    pub description: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CoverageScope {
    UpstreamCatalog,
    SelectedRepositories,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AuthContract {
    pub mode: AuthMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_env: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header_prefix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    Anonymous,
    ExplicitOptIn,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TermsStatus {
    Documented,
    Unverified,
}

pub fn load_sources(root: &Path) -> Result<Vec<SourceDefinition>> {
    let file: SourceFile = store::read_yaml(&root.join("config/sources.yaml"))?;
    ensure!(
        file.schema_version == 1,
        "unsupported source configuration version"
    );
    let expected: BTreeSet<_> = sync::PUBLIC_SOURCES
        .iter()
        .chain(sync::AUTHENTICATED_SOURCES)
        .copied()
        .collect();
    let mut seen = BTreeSet::new();
    for source in &file.sources {
        ensure!(
            expected.contains(source.id.as_str()),
            "unknown source: {}",
            source.id
        );
        ensure!(
            seen.insert(source.id.as_str()),
            "duplicate source: {}",
            source.id
        );
        let public = sync::PUBLIC_SOURCES.contains(&source.id.as_str());
        ensure!(
            public == (source.access == Access::Public)
                && public == (source.auth_contract.mode == AuthMode::Anonymous),
            "source {}: access metadata differs from the adapter contract",
            source.id
        );
        let auth = &source.auth_contract;
        if public {
            ensure!(
                auth.credential_env.is_none()
                    && auth.header_name.is_none()
                    && auth.header_prefix.is_none(),
                "source {}: anonymous contract cannot contain credential fields",
                source.id
            );
        } else {
            let spec = crate::adapters::gated::request_spec(&source.id)
                .context("missing authentication contract")?;
            ensure!(
                auth.credential_env.as_deref() == Some(spec.credential_env)
                    && auth.header_name.as_deref() == Some(spec.header)
                    && auth.header_prefix.as_deref().unwrap_or_default() == spec.prefix,
                "source {}: authentication metadata differs from the adapter contract",
                source.id
            );
        }
        ensure!(
            !source.coverage.description.trim().is_empty() && !source.docs_urls.is_empty(),
            "source {}: coverage and contract documentation are required",
            source.id
        );
        if source.terms_status == TermsStatus::Documented {
            ensure!(
                source
                    .data_license
                    .as_ref()
                    .is_some_and(|license| !license.trim().is_empty())
                    && !source.terms_urls.is_empty(),
                "source {}: documented terms need a license and evidence URL",
                source.id
            );
        }
        for url in source.docs_urls.iter().chain(&source.terms_urls) {
            let url = Url::parse(url).context("invalid source documentation URL")?;
            ensure!(
                url.scheme() == "https"
                    && url.has_host()
                    && url.username().is_empty()
                    && url.password().is_none(),
                "source documentation must use HTTPS without credentials"
            );
        }
    }
    ensure!(
        seen == expected,
        "source configuration must describe all supported sources"
    );
    Ok(file.sources)
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiscoveryConfig {
    pub schema_version: u32,
    pub max_results: usize,
    pub rules: Vec<DiscoveryRule>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "source", rename_all = "lowercase", deny_unknown_fields)]
pub enum DiscoveryRule {
    Huggingface {
        id: String,
        author: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pipeline_tag: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        search: Option<String>,
        sort: HfSort,
        limit: usize,
    },
    Modelscope {
        id: String,
        owner: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        task: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        search: Option<String>,
        sort: MsSort,
        limit: usize,
    },
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub enum HfSort {
    #[serde(rename = "createdAt")]
    CreatedAt,
    #[serde(rename = "lastModified")]
    LastModified,
    #[serde(rename = "downloads")]
    Downloads,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MsSort {
    Default,
    Downloads,
    Likes,
    LastModified,
}

impl DiscoveryRule {
    fn id(&self) -> &str {
        match self {
            Self::Huggingface { id, .. } | Self::Modelscope { id, .. } => id,
        }
    }

    fn source(&self) -> &'static str {
        match self {
            Self::Huggingface { .. } => "huggingface",
            Self::Modelscope { .. } => "modelscope",
        }
    }

    fn owner(&self) -> &str {
        match self {
            Self::Huggingface { author, .. } => author,
            Self::Modelscope { owner, .. } => owner,
        }
    }

    fn limit(&self) -> usize {
        match self {
            Self::Huggingface { limit, .. } | Self::Modelscope { limit, .. } => *limit,
        }
    }

    fn url(&self, page_size: usize) -> Result<String> {
        let mut url = Url::parse(match self {
            Self::Huggingface { .. } => "https://huggingface.co/api/models",
            Self::Modelscope { .. } => "https://modelscope.cn/openapi/v1/models",
        })?;
        {
            let mut query = url.query_pairs_mut();
            match self {
                Self::Huggingface {
                    author,
                    pipeline_tag,
                    search,
                    sort,
                    ..
                } => {
                    query.append_pair("author", author);
                    query.append_pair(
                        "sort",
                        match sort {
                            HfSort::CreatedAt => "createdAt",
                            HfSort::LastModified => "lastModified",
                            HfSort::Downloads => "downloads",
                        },
                    );
                    query.append_pair("direction", "-1");
                    query.append_pair("limit", &page_size.to_string());
                    if let Some(task) = pipeline_tag {
                        query.append_pair("pipeline_tag", task);
                    }
                    if let Some(search) = search {
                        query.append_pair("search", search);
                    }
                }
                Self::Modelscope {
                    owner,
                    task,
                    search,
                    sort,
                    ..
                } => {
                    query.append_pair("owner", owner);
                    query.append_pair(
                        "sort",
                        match sort {
                            MsSort::Default => "default",
                            MsSort::Downloads => "downloads",
                            MsSort::Likes => "likes",
                            MsSort::LastModified => "last_modified",
                        },
                    );
                    query.append_pair("page_number", "1");
                    query.append_pair("page_size", &page_size.to_string());
                    if let Some(task) = task {
                        query.append_pair("filter.task", task);
                    }
                    if let Some(search) = search {
                        query.append_pair("search", search);
                    }
                }
            }
        }
        Ok(url.into())
    }
}

pub fn load_config(root: &Path) -> Result<DiscoveryConfig> {
    let config: DiscoveryConfig = store::read_yaml(&root.join("config/discovery.yaml"))?;
    validate_config(&config)?;
    Ok(config)
}

fn validate_config(config: &DiscoveryConfig) -> Result<()> {
    ensure!(
        config.schema_version == 1,
        "unsupported discovery configuration version"
    );
    ensure!(
        (1..=MAX_TOTAL_RESULTS).contains(&config.max_results),
        "discovery max_results must be 1..=200"
    );
    ensure!(
        !config.rules.is_empty() && config.rules.len() <= MAX_TOTAL_RESULTS,
        "discovery requires 1..=200 rules"
    );
    let mut ids = BTreeSet::new();
    for rule in &config.rules {
        ensure!(
            valid_segment(rule.id()) && ids.insert(rule.id()),
            "invalid or duplicate discovery rule id: {}",
            rule.id()
        );
        ensure!(
            valid_segment(rule.owner()),
            "invalid discovery owner: {}",
            rule.owner()
        );
        ensure!(
            (1..=MAX_RULE_RESULTS).contains(&rule.limit()),
            "rule {}: limit must be 1..=20",
            rule.id()
        );
        let (task, search) = match rule {
            DiscoveryRule::Huggingface {
                pipeline_tag,
                search,
                ..
            } => (pipeline_tag, search),
            DiscoveryRule::Modelscope { task, search, .. } => (task, search),
        };
        for value in task.iter().chain(search) {
            ensure!(
                !value.trim().is_empty()
                    && value.len() <= 200
                    && !value.chars().any(char::is_control),
                "rule {}: invalid search or task value",
                rule.id()
            );
        }
    }
    Ok(())
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DiscoveryReport {
    pub schema_version: u32,
    pub started_at: String,
    pub finished_at: String,
    pub sources: Vec<String>,
    pub max_results: usize,
    pub processed_results: usize,
    pub rules: Vec<RuleReport>,
    pub snapshots: Vec<Snapshot>,
    pub candidates: Vec<Candidate>,
}

impl DiscoveryReport {
    pub fn has_failures(&self) -> bool {
        self.rules
            .iter()
            .any(|rule| rule.status == RuleStatus::Failed)
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RuleReport {
    pub id: String,
    pub source: String,
    pub query_url: String,
    pub status: RuleStatus,
    pub processed_results: usize,
    pub existing_results: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuleStatus {
    Complete,
    Limited,
    Failed,
    Skipped,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Candidate {
    pub source: String,
    pub upstream_id: String,
    pub url: String,
    pub matched_rules: Vec<String>,
    pub evidence: Vec<DiscoveryEvidence>,
}

#[derive(Debug, Serialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord)]
pub struct DiscoveryEvidence {
    pub rule_id: String,
    pub snapshot: String,
    pub pointer: String,
}

pub async fn run(root: &Path, requested: &[String], output: &Path) -> Result<DiscoveryReport> {
    load_sources(root)?;
    let config = load_config(root)?;
    let sources = select_sources(requested)?;
    let mut seeds = BTreeMap::new();
    for source in &sources {
        seeds.insert(
            source.as_str(),
            read_seeds(&root.join("config").join(format!("{source}-models.txt")))?,
        );
    }
    let fetcher = Fetcher::new(&root.join(".cache"))?;
    let mut report = DiscoveryReport {
        schema_version: 1,
        started_at: now()?,
        finished_at: String::new(),
        sources: sources.clone(),
        max_results: config.max_results,
        processed_results: 0,
        rules: Vec::new(),
        snapshots: Vec::new(),
        candidates: Vec::new(),
    };
    let mut candidates = BTreeMap::<(String, String), Candidate>::new();
    let mut snapshots = BTreeMap::new();
    for rule in config
        .rules
        .iter()
        .filter(|rule| sources.iter().any(|source| source == rule.source()))
    {
        let budget = rule
            .limit()
            .min(config.max_results - report.processed_results);
        let page_size = PAGE_SIZE.min(budget.max(1));
        let url = rule.url(page_size)?;
        let collected = collect_rule(
            &fetcher,
            rule,
            &url,
            budget,
            page_size,
            &seeds[rule.source()],
        )
        .await;
        report.processed_results += collected.report.processed_results;
        report.rules.push(collected.report);
        for snapshot in collected.snapshots {
            snapshots.entry(snapshot.id.clone()).or_insert(snapshot);
        }
        for candidate in collected.candidates {
            let entry = candidates
                .entry((candidate.source.clone(), candidate.upstream_id.clone()))
                .or_insert_with(|| Candidate {
                    source: candidate.source.clone(),
                    upstream_id: candidate.upstream_id.clone(),
                    url: candidate.url.clone(),
                    matched_rules: Vec::new(),
                    evidence: Vec::new(),
                });
            entry.matched_rules.extend(candidate.matched_rules);
            entry.evidence.extend(candidate.evidence);
        }
    }
    ensure!(
        !report.rules.is_empty(),
        "no discovery rules match the requested sources"
    );
    for candidate in candidates.values_mut() {
        candidate.matched_rules.sort();
        candidate.matched_rules.dedup();
        candidate.evidence.sort();
        candidate.evidence.dedup();
    }
    report.candidates = candidates.into_values().collect();
    report.snapshots = snapshots.into_values().collect();
    report.finished_at = now()?;
    let output = if output.is_absolute() {
        output.to_owned()
    } else {
        root.join(output)
    };
    let mut bytes = serde_json::to_vec_pretty(&report)?;
    bytes.push(b'\n');
    store::write_bytes(&output, &bytes)?;
    Ok(report)
}

struct Collected {
    report: RuleReport,
    snapshots: Vec<Snapshot>,
    candidates: Vec<Candidate>,
}

async fn collect_rule(
    fetcher: &Fetcher,
    rule: &DiscoveryRule,
    initial_url: &str,
    budget: usize,
    page_size: usize,
    seeds: &BTreeSet<String>,
) -> Collected {
    let mut collected = Collected {
        report: RuleReport {
            id: rule.id().into(),
            source: rule.source().into(),
            query_url: initial_url.into(),
            status: if budget == 0 {
                RuleStatus::Skipped
            } else {
                RuleStatus::Complete
            },
            processed_results: 0,
            existing_results: 0,
            total_count: None,
            error: None,
        },
        snapshots: Vec::new(),
        candidates: Vec::new(),
    };
    if budget == 0 {
        return collected;
    }
    let result: Result<()> = async {
        let mut url = initial_url.to_owned();
        let mut visited = BTreeSet::new();
        let mut page = 1usize;
        loop {
            ensure!(
                visited.insert(url.clone()) && visited.len() <= MAX_RULE_RESULTS,
                "discovery pagination repeated or exceeded its page budget"
            );
            let (snapshot, value, link) = fetcher
                .get_json_with_link(rule.source(), &url, HeaderMap::new())
                .await?;
            collected.snapshots.push(snapshot.clone());
            let (items, pointer, next) = match rule {
                DiscoveryRule::Huggingface { .. } => {
                    let items = value
                        .as_array()
                        .context("Hugging Face list response must be an array")?;
                    (items, "", link)
                }
                DiscoveryRule::Modelscope { .. } => {
                    ensure!(
                        value.get("success").and_then(Value::as_bool) == Some(true),
                        "ModelScope list response must have success=true"
                    );
                    let items = value
                        .pointer("/data/models")
                        .and_then(Value::as_array)
                        .context("ModelScope list response must contain data.models")?;
                    let total = value
                        .pointer("/data/total_count")
                        .and_then(Value::as_u64)
                        .context("ModelScope list response must contain nonnegative total_count")?;
                    collected.report.total_count = Some(total);
                    ensure!(
                        items.len() <= page_size,
                        "ModelScope page exceeds page_size"
                    );
                    let end = ((page - 1) * page_size + items.len()) as u64;
                    ensure!(end <= total, "ModelScope page exceeds total_count");
                    let next = if end < total {
                        ensure!(
                            items.len() == page_size,
                            "ModelScope pagination returned a short unfinished page"
                        );
                        ensure!(
                            (page + 1) * page_size <= 3000,
                            "ModelScope pagination exceeds the documented window"
                        );
                        let mut next = Url::parse(&url)?;
                        let params: Vec<_> = next
                            .query_pairs()
                            .filter(|(key, _)| key != "page_number")
                            .map(|(key, value)| (key.into_owned(), value.into_owned()))
                            .collect();
                        next.query_pairs_mut()
                            .clear()
                            .extend_pairs(params)
                            .append_pair("page_number", &(page + 1).to_string());
                        Some(next.into())
                    } else {
                        None
                    };
                    (items, "/data/models", next)
                }
            };
            ensure!(
                !items.is_empty() || next.is_none(),
                "discovery returned an empty unfinished page"
            );
            let remaining = budget - collected.report.processed_results;
            for (index, item) in items.iter().take(remaining).enumerate() {
                collected.report.processed_results += 1;
                let id = item
                    .get("id")
                    .and_then(Value::as_str)
                    .context("discovery item has no string id")?;
                ensure!(valid_repo_id(id), "invalid discovered repository id: {id}");
                ensure!(
                    id.split('/')
                        .next()
                        .is_some_and(|owner| owner.eq_ignore_ascii_case(rule.owner())),
                    "discovery response escaped the configured owner: {id}"
                );
                if seeds.contains(id) {
                    collected.report.existing_results += 1;
                    continue;
                }
                collected.candidates.push(Candidate {
                    source: rule.source().into(),
                    upstream_id: id.into(),
                    url: format!(
                        "{}{id}",
                        if rule.source() == "huggingface" {
                            "https://huggingface.co/"
                        } else {
                            "https://modelscope.cn/models/"
                        }
                    ),
                    matched_rules: vec![rule.id().into()],
                    evidence: vec![DiscoveryEvidence {
                        rule_id: rule.id().into(),
                        snapshot: snapshot.id.clone(),
                        pointer: format!("{pointer}/{index}"),
                    }],
                });
            }
            if items.len() > remaining
                || (collected.report.processed_results == budget && next.is_some())
            {
                collected.report.status = RuleStatus::Limited;
                break;
            }
            match next {
                Some(next) => {
                    url = checked_next_url(&url, &next)?;
                    page += 1;
                }
                None => break,
            }
        }
        Ok(())
    }
    .await;
    if let Err(error) = result {
        collected.report.status = RuleStatus::Failed;
        collected.report.error = Some(format!("{error:#}"));
    }
    collected
}

fn select_sources(requested: &[String]) -> Result<Vec<String>> {
    let sources: Vec<String> = if requested.is_empty() {
        DISCOVERY_SOURCES
            .iter()
            .map(|source| (*source).into())
            .collect()
    } else {
        requested.to_vec()
    };
    let mut seen = BTreeSet::new();
    for source in &sources {
        ensure!(
            DISCOVERY_SOURCES.contains(&source.as_str()),
            "source {source} has no public discovery adapter"
        );
        ensure!(seen.insert(source), "duplicate discovery source: {source}");
    }
    Ok(sources)
}

fn read_seeds(path: &Path) -> Result<BTreeSet<String>> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let mut seeds = BTreeSet::new();
    for id in text
        .lines()
        .map(str::trim)
        .filter(|id| !id.is_empty() && !id.starts_with('#'))
    {
        ensure!(
            valid_repo_id(id) && seeds.insert(id.to_owned()),
            "invalid or duplicate seed id: {id}"
        );
    }
    Ok(seeds)
}

fn valid_segment(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
}

fn valid_repo_id(value: &str) -> bool {
    value
        .split_once('/')
        .is_some_and(|(owner, name)| valid_segment(owner) && valid_segment(name))
}

fn now() -> Result<String> {
    Ok(OffsetDateTime::now_utc().format(&Rfc3339)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
        time::Duration,
    };

    fn hf_rule() -> DiscoveryRule {
        DiscoveryRule::Huggingface {
            id: "test".into(),
            author: "org".into(),
            pipeline_tag: Some("image-segmentation".into()),
            search: None,
            sort: HfSort::CreatedAt,
            limit: 20,
        }
    }

    fn server(
        responses: Vec<(u16, &'static str, &'static str)>,
    ) -> (String, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/models", listener.local_addr().unwrap());
        let worker = thread::spawn(move || {
            responses.into_iter().map(|(status, headers, body)| {
            let (mut socket, _) = listener.accept().unwrap();
            socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut bytes = Vec::new();
            let mut byte = [0];
            while !bytes.ends_with(b"\r\n\r\n") {
                socket.read_exact(&mut byte).unwrap();
                bytes.extend(byte);
            }
            write!(socket, "HTTP/1.1 {status} Test\r\nConnection: close\r\nContent-Length: {}\r\n{headers}\r\n{body}", body.len()).unwrap();
            String::from_utf8(bytes).unwrap()
        }).collect()
        });
        (url, worker)
    }

    #[test]
    fn configuration_limits_and_source_metadata_are_validated() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        assert_eq!(load_sources(root).unwrap().len(), 12);
        let config = load_config(root).unwrap();
        let mut value = serde_json::to_value(config).unwrap();
        value["max_results"] = json!(201);
        assert!(validate_config(&serde_json::from_value(value.clone()).unwrap()).is_err());
        value["max_results"] = json!(200);
        value["rules"][0]["limit"] = json!(21);
        assert!(validate_config(&serde_json::from_value(value.clone()).unwrap()).is_err());
        value["rules"][0]["limit"] = json!(20);
        value["rules"][0]["endpoint"] = json!("https://other.test");
        assert!(serde_json::from_value::<DiscoveryConfig>(value).is_err());
        assert!(select_sources(&["openai".into()]).is_err());
        assert!(select_sources(&["huggingface".into(), "huggingface".into()]).is_err());
        assert!(!valid_repo_id("../model"));
        assert!(!valid_repo_id("org/a/b"));
        assert!(!valid_repo_id("org/a?token=x"));
    }

    #[tokio::test]
    async fn hf_pages_produce_new_candidates_with_original_evidence() {
        let (url, worker) = server(vec![
            (
                200,
                "Link: </models?cursor=a,b>; rel=next\r\n",
                r#"[{"id":"org/old"}]"#,
            ),
            (200, "", r#"[{"id":"org/new"}]"#),
        ]);
        let cache = tempfile::tempdir().unwrap();
        let fetcher = Fetcher::new(cache.path()).unwrap();
        let collected = collect_rule(
            &fetcher,
            &hf_rule(),
            &url,
            20,
            10,
            &BTreeSet::from(["org/old".into()]),
        )
        .await;
        assert_eq!(collected.report.status, RuleStatus::Complete);
        assert_eq!(collected.report.processed_results, 2);
        assert_eq!(collected.report.existing_results, 1);
        assert_eq!(collected.candidates.len(), 1);
        let evidence = &collected.candidates[0].evidence[0];
        assert_eq!(evidence.snapshot, collected.snapshots[1].id);
        let raw: Value = serde_json::from_slice(
            &fs::read(
                cache
                    .path()
                    .join("snapshots")
                    .join(format!("{}.json", collected.snapshots[1].sha256)),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(raw.pointer(&evidence.pointer).unwrap()["id"], "org/new");
        let requests = worker.join().unwrap();
        assert!(requests[1].contains("cursor=a,b"));
        for request in requests {
            let request = request.to_ascii_lowercase();
            assert!(!request.contains("authorization:") && !request.contains("cookie:"));
        }
    }

    #[tokio::test]
    async fn budgets_stop_processing_and_failed_rules_keep_their_evidence() {
        let (url, worker) = server(vec![(
            200,
            "",
            r#"[{"id":"org/a"},{"id":"org/b"},{"id":"org/c"}]"#,
        )]);
        let cache = tempfile::tempdir().unwrap();
        let fetcher = Fetcher::new(cache.path()).unwrap();
        let collected = collect_rule(&fetcher, &hf_rule(), &url, 2, 2, &BTreeSet::new()).await;
        assert_eq!(collected.report.status, RuleStatus::Limited);
        assert_eq!(collected.report.processed_results, 2);
        assert_eq!(collected.candidates.len(), 2);
        worker.join().unwrap();
        let skipped = collect_rule(
            &fetcher,
            &hf_rule(),
            "https://example.invalid/",
            0,
            1,
            &BTreeSet::new(),
        )
        .await;
        assert_eq!(skipped.report.status, RuleStatus::Skipped);
        assert!(skipped.snapshots.is_empty());

        let (url, worker) = server(vec![
            (
                200,
                "Link: </models?cursor=next>; rel=next\r\n",
                r#"[{"id":"org/a"}]"#,
            ),
            (401, "", "{}"),
        ]);
        let collected = collect_rule(&fetcher, &hf_rule(), &url, 20, 10, &BTreeSet::new()).await;
        assert_eq!(collected.report.status, RuleStatus::Failed);
        assert!(collected.report.error.unwrap().contains("401"));
        assert_eq!(collected.candidates.len(), 1);
        assert_eq!(collected.snapshots.len(), 1);
        worker.join().unwrap();
    }

    #[tokio::test]
    async fn modelscope_pagination_preserves_filters_and_rejects_owner_mismatch() {
        let rule = DiscoveryRule::Modelscope {
            id: "ms".into(),
            owner: "org".into(),
            task: Some("text-generation".into()),
            search: None,
            sort: MsSort::LastModified,
            limit: 20,
        };
        let official = Url::parse(&rule.url(10).unwrap()).unwrap();
        assert_eq!(official.path(), "/openapi/v1/models");
        assert!(
            official
                .query_pairs()
                .any(|(key, value)| key == "filter.task" && value == "text-generation")
        );
        let (url, worker) = server(vec![
            (
                200,
                "",
                r#"{"success":true,"data":{"models":[{"id":"org/a"}],"total_count":2}}"#,
            ),
            (
                200,
                "",
                r#"{"success":true,"data":{"models":[{"id":"other/b"}],"total_count":2}}"#,
            ),
        ]);
        let url = format!("{url}?owner=org&filter.task=text-generation&page_number=1&page_size=1");
        let cache = tempfile::tempdir().unwrap();
        let fetcher = Fetcher::new(cache.path()).unwrap();
        let collected = collect_rule(&fetcher, &rule, &url, 20, 1, &BTreeSet::new()).await;
        assert_eq!(collected.report.status, RuleStatus::Failed);
        assert!(collected.report.error.unwrap().contains("configured owner"));
        assert_eq!(collected.snapshots.len(), 2);
        assert_eq!(collected.candidates.len(), 1);
        let requests = worker.join().unwrap();
        assert!(requests[1].contains("filter.task=text-generation"));
        assert!(requests[1].contains("page_number=2"));
    }

    #[tokio::test]
    async fn modelscope_rejects_inconsistent_page_sizes_and_accepts_short_final_pages() {
        let rule = DiscoveryRule::Modelscope {
            id: "ms".into(),
            owner: "org".into(),
            task: None,
            search: None,
            sort: MsSort::LastModified,
            limit: 20,
        };
        let unfinished = r#"{"success":true,"data":{"models":[{"id":"org/a"},{"id":"org/b"},{"id":"org/c"},{"id":"org/d"},{"id":"org/e"}],"total_count":20}}"#;
        let final_page = r#"{"success":true,"data":{"models":[{"id":"org/a"},{"id":"org/b"},{"id":"org/c"},{"id":"org/d"},{"id":"org/e"}],"total_count":5}}"#;
        for (page_size, body, expected_error) in [
            (10, unfinished, Some("short unfinished page")),
            (4, final_page, Some("exceeds page_size")),
            (10, final_page, None),
        ] {
            let (url, worker) = server(vec![(200, "", body)]);
            let cache = tempfile::tempdir().unwrap();
            let fetcher = Fetcher::new(cache.path()).unwrap();
            let collected =
                collect_rule(&fetcher, &rule, &url, 20, page_size, &BTreeSet::new()).await;
            match expected_error {
                Some(error) => {
                    assert_eq!(collected.report.status, RuleStatus::Failed);
                    assert!(collected.report.error.unwrap().contains(error));
                    assert!(collected.candidates.is_empty());
                }
                None => {
                    assert_eq!(collected.report.status, RuleStatus::Complete);
                    assert_eq!(collected.candidates.len(), 5);
                }
            }
            assert_eq!(collected.snapshots.len(), 1);
            assert_eq!(worker.join().unwrap().len(), 1);
        }
    }
}
