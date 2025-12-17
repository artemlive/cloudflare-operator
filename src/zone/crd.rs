use crate::cloudflare::{AccountScoped, HasSecretRef};
use k8s_openapi::api::core::v1::{LocalObjectReference, SecretKeySelector};
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[cfg_attr(test, derive(Default))]
#[kube(kind = "Zone", group = "cloudflare.com", version = "v1alpha1", namespaced)]
#[kube(status = "ZoneStatus", shortname = "zone")]
pub struct ZoneSpec {
    pub account_ref: Option<LocalObjectReference>,
    pub secret_ref: Option<SecretKeySelector>,
}

impl HasSecretRef for ZoneSpec {
    fn secret_ref(&self) -> &Option<SecretKeySelector> {
        &self.secret_ref
    }
}

impl AccountScoped for ZoneSpec {
    fn account_ref(&self) -> &Option<LocalObjectReference> {
        &self.account_ref
    }
}

#[derive(Deserialize, Serialize, Clone, Default, Debug, JsonSchema)]
pub struct ZoneStatus {
    pub ready: bool,
    pub zone_id: String,
}
