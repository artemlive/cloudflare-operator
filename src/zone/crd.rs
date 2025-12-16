use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[cfg_attr(test, derive(Default))]
#[kube(kind = "Zone", group = "cloudflare.com", version = "v1alpha1", namespaced)]
#[kube(status = "ZoneStatus", shortname = "zone")]
pub struct ZoneSpec {
    pub account: String,
}

#[derive(Deserialize, Serialize, Clone, Default, Debug, JsonSchema)]
pub struct ZoneStatus {
    pub ready: bool,
}
