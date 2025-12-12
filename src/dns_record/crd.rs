use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[cfg_attr(test, derive(Default))]
#[kube(
    kind = "DNSRecord",
    group = "cloudflare.com",
    version = "v1alpha1",
    namespaced
)]
#[kube(status = "DNSRecordStatus", shortname = "dns")]
pub struct DNSRecordSpec {
    pub zone_id: String,
    pub name: String,
    pub record_type: String,
    pub content: String,
    pub ttl: Option<u32>,
    pub priority: Option<u16>,
    pub proxied: Option<bool>,
}

#[derive(Deserialize, Serialize, Clone, Default, Debug, JsonSchema)]
pub struct DNSRecordStatus {
    pub ready: bool,
    pub record_id: Option<String>,
}
