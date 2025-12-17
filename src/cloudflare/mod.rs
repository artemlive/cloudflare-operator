use crate::{account::Account, cf_client::CloudflareClient, zone::Zone};
use k8s_openapi::api::core::v1::{LocalObjectReference, Secret, SecretKeySelector};
use kube::{Api, Client, ResourceExt};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;

#[derive(Debug)]
pub enum ZoneKind {
    Zone,
    ClusterZone,
}

#[derive(Debug)]
pub enum AccountKind {
    Account,
    ClusterAccount,
}

pub trait HasSecretRef {
    fn secret_ref(&self) -> &Option<SecretKeySelector>;
}
pub trait AccountScoped {
    fn account_ref(&self) -> &Option<LocalObjectReference>;
}
pub trait ZoneScoped {
    fn zone_ref(&self) -> &LocalObjectReference;
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

    pub async fn get_client<T>(&self, resource: &T, namespace: &str) -> Result<Arc<CloudflareClient>, Error>
    where
        T: ZoneScoped,
    {
        let token = self
            .resolve_token_for_zone(resource.zone_ref(), namespace)
            .await?;
        self.get_client_from_cache(token).await
    }

    pub async fn get_client<T>(&self, resource: &T, namespace: &str) -> Result<Arc<CloudflareClient>, Error>
    where
        T: AccountScoped,
    {
        let token = self
            .resolve_token_for_account(resource.account_ref(), namespace)
            .await?;
        self.get_client_from_cache(token).await
    }

    async fn get_client_from_cache(&self, token: String) -> Result<Arc<cf_client::CloudflareClient>, Error> {
        let mut cache = self.cache.lock().await;

        if let Some(client) = cache.get(&token) {
            return Ok(client.clone());
        }

        let arc_client =
            Arc::new(cf_client::CloudflareClient::new(&token).map_err(|e| Error::ClientCreation(e))?);
        cache.insert(token, arc_client.clone());

        Ok(arc_client)
    }

    async fn resolve_token_for_zone<T>(&self, zone_ref: &ZoneRef, namespace: &str) -> Result<String, Error>
    where
        T: ZoneScoped,
    {
        let zone: Api<Zone> = Api::namespaced(self.k8s_client.clone(), namespace);

        if let Some(token) = self.read_secret(&zone, namespace).await? {
            return Ok(token);
        }

        self.resolve_token_for_account(&zone, namespace)
    }

    async fn resolve_token_for_account<T>(
        &self,
        resource: &T,
        namespace: &str,
    ) -> Result<Option<String>, Error>
    where
        T: AccountScoped,
    {
        if let Some(account_ref) = resource.account_ref() {
            let account: Api<Account> = Api::namespaced(self.k8s_client.clone(), &namespace);
            if let Some(token) = self.read_secret(&account, &namespace).await? {
                return Ok(token);
            }
        }

        Ok(Some(self.default_token.clone()))
    }

    async fn read_secret<T>(&self, resource: &T, namespace: &str) -> Result<String, Error>
    where
        T: HasSecretRef,
    {
        if let Some(secret_ref) = resource.secret_ref() {
            let secrets: Api<Secret> = Api::namespaced(self.k8s_client.clone(), namespace);
            let secret = secrets
                .get(&secret_ref.name)
                .await
                .map_err(|_| Error::SecretNotFound)?;

            if let Some(data) = secret.data {
                if let Some(byte_token) = data.get(&secret_ref.key) {
                    return Some(String::from_utf8(byte_token.0.clone()).map_err(|_| Error::TokenEncoding));
                }
            }
            Err(Error::SecretKeyMissing)
        }
        Ok(None)
    }
}
