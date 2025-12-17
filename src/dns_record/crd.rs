use crate::cloudflare::ZoneScoped;
use k8s_openapi::api::core::v1::LocalObjectReference;
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
    pub zone_ref: LocalObjectReference,
    pub name: String,
    pub record_type: String,
    pub content: String,
    pub ttl: Option<u32>,
    pub priority: Option<u16>,
    pub proxied: Option<bool>,
}

impl ZoneScoped for DNSRecordSpec {
    fn zone_ref(&self) -> &LocalObjectReference {
        &self.zone_ref
    }
}


#[derive(Deserialize, Serialize, Clone, Default, Debug, JsonSchema)]
pub struct DNSRecordStatus {
    pub ready: bool,
    pub record_id: Option<String>,
}
