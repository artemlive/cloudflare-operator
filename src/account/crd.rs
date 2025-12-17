use k8s_openapi::api::core::v1::SecretKeySelector;
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::cloudflare::CloudflareResource;

#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[cfg_attr(test, derive(Default))]
#[kube(kind = "Account", group = "cloudflare.com", version = "v1alpha1", namespaced)]
#[kube(status = "AccountStatus", shortname = "acc")]
pub struct AccountSpec {
    pub secret_ref: Option<SecretKeySelector>,
}

impl CloudflareResource for Account {
    fn secret_ref(&self) -> Option<&SecretKeySelector> {
        self.spec.secret_ref.as_ref()
    }
}

#[derive(Deserialize, Serialize, Clone, Default, Debug, JsonSchema)]
pub struct AccountStatus {
    pub ready: bool,
    pub id: Option<String>,
}
