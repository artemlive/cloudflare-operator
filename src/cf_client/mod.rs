use std::sync::Arc;
// re-export the types, I feel like it's fine
pub use cloudflare::endpoints::{
    account::{Account, GetAccount},
    dns::dns::{CreateDnsRecordParams, DnsContent},
    zones::zone::{CreateZone, CreateZoneParams, Zone, ZoneDetails},
};

use cloudflare::{
    endpoints::{account::ListAccounts, dns::dns},
    framework::{
        Environment, auth,
        client::{ClientConfig, async_api},
    },
};

pub struct CloudflareClient {
    client: Arc<async_api::Client>,
}

use anyhow::Result;
impl CloudflareClient {
    pub fn new(token: String) -> Result<Self> {
        let credentials = auth::Credentials::UserAuthToken { token };
        let api_client =
            async_api::Client::new(credentials, ClientConfig::default(), Environment::Production)?;

        Ok(Self {
            client: Arc::new(api_client),
        })
    }

    pub async fn create_dns_record(
        &self,
        zone_id: &str,
        dns_params: CreateDnsRecordParams<'_>, // we need the lifetime, because we have the
                                               // reference in the params, so we need to make sure
                                               // that it outlives the params itself
    ) -> Result<String> {
        let endpoint = dns::CreateDnsRecord {
            zone_identifier: zone_id,
            params: dns_params,
        };
        let response = self.client.request(&endpoint).await?;
        Ok(response.result.id)
    }

    pub async fn create_zone(&self, params: CreateZoneParams<'_>) -> Result<String> {
        Ok(self.client.request(&CreateZone { params }).await?.result.id)
    }

    pub async fn get_zone(&self, identifier: &str) -> Result<Zone> {
        Ok(self.client.request(&ZoneDetails { identifier }).await?.result)
    }

    pub async fn get_account(&self, identifier: &str) -> Result<Account> {
        Ok(self.client.request(&GetAccount { identifier }).await?.result)
    }

    pub async fn list_account(&self) -> Result<Vec<Account>> {
        Ok(self.client.request(&ListAccounts { params: None }).await?.result)
    }
}

impl Clone for CloudflareClient {
    fn clone(&self) -> Self {
        Self {
            client: Arc::clone(&self.client),
        }
    }
}
