use crate::{account::Account, cf_client::CloudflareClient, zone::Zone};
use async_recursion::async_recursion;
use k8s_openapi::api::core::v1::{LocalObjectReference, Secret, SecretKeySelector};
use kube::{Api, Client, ResourceExt};
use std::{collections::HashMap, sync::Arc};
use thiserror::Error;
use tokio::sync::Mutex;

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("Secret {0} not found")]
    SecretNotFound(String),
    #[error("Zone {0} not found")]
    ZoneNotFound(String),
    #[error("Account {0} not found")]
    AccountNotFound(String),
    #[error("Secret key {0} missing")]
    SecretKeyMissing(String),
    #[error("Token encoding error")]
    TokenEncoding,
    #[error("Client creation error: {0}")]
    ClientCreation(String),
    #[error("K8s error: {0}")]
    K8sError(#[from] kube::Error),
}

pub trait CloudflareResource {
    fn secret_ref(&self) -> Option<&SecretKeySelector> {
        None
    }

    fn zone_ref(&self) -> Option<&LocalObjectReference> {
        None
    }

    fn account_ref(&self) -> Option<&LocalObjectReference> {
        None
    }
}

type ClientCache = Arc<Mutex<HashMap<String, Arc<CloudflareClient>>>>;

#[derive(Clone)]
pub struct CloudflareClientProvider {
    k8s_client: Client,
    default_token: String,
    cache: ClientCache,
}

impl CloudflareClientProvider {
    pub fn new(k8s_client: Client, default_token: String) -> Self {
        Self {
            k8s_client,
            default_token,
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn get_client<T>(
        &self,
        resource: &T,
        namespace: &str,
    ) -> Result<Arc<CloudflareClient>, ProviderError>
    where
        T: CloudflareResource + ResourceExt + Sync + Send,
    {
        let token = self.resolve_token(resource, namespace).await?;
        self.get_client_from_cache(token).await
    }

    async fn get_client_from_cache(&self, token: String) -> Result<Arc<CloudflareClient>, ProviderError> {
        let mut cache = self.cache.lock().await;

        if let Some(client) = cache.get(&token) {
            return Ok(client.clone());
        }

        let arc_client = Arc::new(
            CloudflareClient::new(token.clone()).map_err(|e| ProviderError::ClientCreation(e.to_string()))?,
        );
        cache.insert(token, arc_client.clone());

        Ok(arc_client)
    }

    #[async_recursion]
    async fn resolve_token<T>(&self, resource: &T, namespace: &str) -> Result<String, ProviderError>
    where
        T: CloudflareResource + Sync + Send,
    {
        if let Some(s_ref) = resource.secret_ref() {
            return self.fetch_secret::<T>(s_ref, namespace).await;
        }

        if let Some(z_ref) = resource.zone_ref() {
            let zone: Api<Zone> = Api::namespaced(self.k8s_client.clone(), namespace);
            return self
                .resolve_token(
                    &zone
                        .get(&z_ref.name)
                        .await
                        .map_err(|_| ProviderError::ZoneNotFound(z_ref.name.clone()))?,
                    &namespace,
                )
                .await;
        }

        if let Some(a_ref) = resource.zone_ref() {
            let account: Api<Account> = Api::namespaced(self.k8s_client.clone(), namespace);
            return self
                .resolve_token(
                    &account
                        .get(&a_ref.name)
                        .await
                        .map_err(|_| ProviderError::AccountNotFound(a_ref.name.clone()))?,
                    &namespace,
                )
                .await;
        }

        Ok(self.default_token.clone())
    }

    async fn fetch_secret<T>(
        &self,
        secret_ref: &SecretKeySelector,
        namespace: &str,
    ) -> Result<String, ProviderError> {
        let secrets: Api<Secret> = Api::namespaced(self.k8s_client.clone(), namespace);
        let secret = secrets
            .get(&secret_ref.name)
            .await
            .map_err(|_| ProviderError::SecretNotFound(secret_ref.name.clone()))?;

        if let Some(data) = secret.data {
            if let Some(byte_token) = data.get(&secret_ref.key) {
                return String::from_utf8(byte_token.0.clone()).map_err(|_| ProviderError::TokenEncoding);
            }
        }
        Err(ProviderError::SecretKeyMissing(secret_ref.key.clone()))
    }
}
