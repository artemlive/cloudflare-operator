use k8s_openapi::api::core::v1::{LocalObjectReference, SecretKeySelector};
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::cloudflare::CloudflareResource;

#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[cfg_attr(test, derive(Default))]
#[kube(kind = "Zone", group = "cloudflare.com", version = "v1alpha1", namespaced)]
#[kube(status = "ZoneStatus", shortname = "zone")]
#[serde(rename_all = "camelCase")]
pub struct ZoneSpec {
    pub account_ref: Option<LocalObjectReference>,
    pub secret_ref: Option<SecretKeySelector>,
}

impl CloudflareResource for Zone {
    fn secret_ref(&self) -> Option<&SecretKeySelector> {
        self.spec.secret_ref.as_ref()
    }

    fn account_ref(&self) -> Option<&LocalObjectReference> {
        self.spec.account_ref.as_ref()
    }
}

#[derive(Deserialize, Serialize, Clone, Default, Debug, JsonSchema)]
pub struct ZoneStatus {
    pub ready: bool,
    pub id: Option<String>,
    pub error: Option<String>,
}
