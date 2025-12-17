use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[kube(kind = "PageRule", group = "cloudflare.com", version = "v1alpha1", namespaced)]
#[kube(status = "PageRuleStatus", shortname = "pr")]
pub struct PageRuleSpec {
    /// The Cloudflare zone ID this page rule belongs to
    pub zone_id: String,

    /// The set of actions to perform if the targets match the request
    pub actions: Vec<PageRuleAction>,

    /// The priority of the rule (higher number = higher priority)
    pub priority: i64,

    /// The status of the page rule
    #[serde(default = "default_status")]
    pub status: PageRuleStatusType,

    /// The rule targets to evaluate on each request
    pub targets: Vec<PageRuleTarget>,
}

fn default_status() -> PageRuleStatusType {
    PageRuleStatusType::Active
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct PageRuleAction {
    /// The action ID
    pub id: PageRuleActionID,

    /// The action value - type depends on the action ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<PageRuleActionValue>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[serde(untagged)]
pub enum PageRuleActionValue {
    Bool(bool),
    Int(i64),
    String(String),
    ForwardingURL(ForwardingURLValue),
    CacheTTLByStatus(HashMap<String, i64>),
    CacheKeyFields(CacheKeyFieldsValue),
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct ForwardingURLValue {
    pub url: String,
    pub status_code: i64,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct CacheKeyFieldsValue {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query_string: Option<CacheKeyQueryString>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub header: Option<CacheKeyHeader>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub cookie: Option<CacheKeyCookie>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<CacheKeyHost>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<CacheKeyUser>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct CacheKeyQueryString {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclude: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct CacheKeyHeader {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclude: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub check_presence: Option<Vec<String>>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct CacheKeyCookie {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclude: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub check_presence: Option<Vec<String>>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct CacheKeyHost {
    pub resolved: bool,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct CacheKeyUser {
    pub device_type: bool,
    pub geo: bool,
    pub lang: bool,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct PageRuleTarget {
    /// The target type
    pub target: PageRuleTargetType,

    /// The constraint for this target
    pub constraint: PageRuleConstraint,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct PageRuleConstraint {
    /// The operator
    pub operator: PageRuleOperator,

    /// The value to match against
    pub value: String,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PageRuleTargetType {
    Url,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PageRuleOperator {
    Matches,
    Contains,
    Equals,
    NotEquals,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PageRuleActionID {
    AlwaysUseHttps,
    AutomaticHttpsRewrites,
    BrowserCacheTtl,
    BrowserCheck,
    BypassCacheOnCookie,
    CacheByDeviceType,
    CacheDeceptionArmor,
    CacheKeyFields,
    CacheLevel,
    CacheOnCookie,
    CacheTtlByStatus,
    DisableApps,
    DisablePerformance,
    DisableSecurity,
    DisableZaraz,
    EdgeCacheTtl,
    EmailObfuscation,
    ExplicitCacheControl,
    ForwardingUrl,
    HostHeaderOverride,
    IpGeolocation,
    Mirage,
    OpportunisticEncryption,
    OriginErrorPagePassThru,
    Polish,
    ResolveOverride,
    RespectStrongEtag,
    ResponseBuffering,
    RocketLoader,
    SecurityLevel,
    SortQueryStringForCache,
    Ssl,
    TrueClientIpHeader,
    Waf,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PageRuleStatusType {
    Active,
    Disabled,
}

#[derive(Deserialize, Serialize, Clone, Default, Debug, JsonSchema)]
pub struct PageRuleStatus {
    /// Whether the page rule is ready
    pub ready: bool,

    /// The Cloudflare page rule ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<String>,

    /// When the rule was created in Cloudflare
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_on: Option<String>,

    /// When the rule was last modified in Cloudflare
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_on: Option<String>, // ISO 8601 string
}
