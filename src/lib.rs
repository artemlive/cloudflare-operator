use std::sync::Arc;

use serde::Serialize;
use thiserror::Error;

use chrono::{DateTime, Utc};
use kube::{
    client::Client,
    runtime::events::{Recorder, Reporter},
};

use cloudflare::CloudflareClientProvider;
use tokio::sync::RwLock;
#[derive(Error, Debug)]
pub enum Error {
    #[error("SerializationError: {0}")]
    SerializationError(#[source] serde_json::Error),

    #[error("Kube Error: {0}")]
    KubeError(#[source] kube::Error),

    #[error("Finalizer Error: {0}")]
    // NB: awkward type because finalizer::Error embeds the reconciler error (which is this)
    // so boxing this error to break cycles
    FinalizerError(#[source] Box<kube::runtime::finalizer::Error<Error>>),

    #[error("IllegalDocument")]
    IllegalDocument,

    #[error("Invalid IP address: {0}")]
    InvalidIpAddress(#[from] std::net::AddrParseError),

    #[error("Unsupported record type: {0}")]
    UnsupportedRecordType(String),

    #[error("Cloudflare API error: {0}")]
    CloudflareApiError(#[from] anyhow::Error),
}
pub type Result<T, E = Error> = std::result::Result<T, E>;

impl Error {
    pub fn metric_label(&self) -> String {
        format!("{self:?}").to_lowercase()
    }
}

/// Diagnostics to be exposed by the web server
#[derive(Clone, Serialize)]
pub struct Diagnostics {
    #[serde(deserialize_with = "from_ts")]
    pub last_event: DateTime<Utc>,
    #[serde(skip)]
    pub reporter: Reporter,
}
impl Default for Diagnostics {
    fn default() -> Self {
        Self {
            last_event: Utc::now(),
            reporter: "doc-controller".into(),
        }
    }
}
impl Diagnostics {
    fn recorder(&self, client: Client) -> Recorder {
        Recorder::new(client, self.reporter.clone())
    }
}

/// State shared between the controller and the web server
#[derive(Clone)]
pub struct State {
    /// Diagnostics populated by the reconciler
    diagnostics: Arc<RwLock<Diagnostics>>,
    /// Metrics
    metrics: Arc<Metrics>,
}

/// State wrapper around the controller outputs for the web server
impl State {
    pub fn new() -> Self {
        Self {
            diagnostics: Arc::default(),
            metrics: Arc::default(),
        }
    }

    /// Metrics getter
    pub fn metrics(&self) -> String {
        let mut buffer = String::new();
        let registry = &*self.metrics.registry;
        prometheus_client::encoding::text::encode(&mut buffer, registry).unwrap();
        buffer
    }

    /// State getter
    pub async fn diagnostics(&self) -> Diagnostics {
        self.diagnostics.read().await.clone()
    }

    // Create a Controller Context that can update State
    pub async fn to_context(&self, client: Client, token: String) -> Arc<Context> {
        Arc::new(Context {
            client: client.clone(),
            recorder: self.diagnostics.read().await.recorder(client.clone()),
            metrics: self.metrics.clone(),
            diagnostics: self.diagnostics.clone(),
            provider: CloudflareClientProvider::new(client, token),
        })
    }
}

// Context for our reconciler
#[derive(Clone)]
pub struct Context {
    /// Kubernetes client
    pub client: Client,
    /// Event recorder
    pub recorder: Recorder,
    /// Diagnostics read by the web server
    pub diagnostics: Arc<RwLock<Diagnostics>>,
    /// Prometheus metrics
    pub metrics: Arc<Metrics>,
    pub provider: CloudflareClientProvider,
}

pub async fn run(state: State) {
    tokio::select! {
        _ = dns_record::run(state.clone()) => {}
        _ = zone::run(state.clone()) => {}
        _ = account::run(state.clone()) => {}
        // in future we could run other workers here future: _ = worker::run(state.clone()) => {},
    }
}
/// Log and trace integrations
pub mod telemetry;

/// Metrics
mod metrics;
pub use metrics::Metrics;
pub mod account;
pub mod cf_client;
pub mod cloudflare;
pub mod dns_record;
pub mod page_rule;
pub mod zone;
//TODO: reanimate tests
//#[cfg(test)]
//pub mod fixtures;
