use crate::{
    Context, Error, Result, State,
    cf_client::{self, CreateZoneParams},
    telemetry,
    zone::{Zone, ZoneStatus},
};
use chrono::Utc;
use futures::StreamExt;
use kube::{
    Resource,
    api::{Api, ListParams, Patch, PatchParams, ResourceExt},
    client::Client,
    runtime::{
        controller::{Action, Controller},
        events::{Event, EventType},
        finalizer::{Event as Finalizer, finalizer},
        watcher::Config,
    },
};
use serde_json::json;
use std::sync::Arc;
use tokio::time::Duration;
use tracing::*;
pub static DOCUMENT_FINALIZER: &str = "zone.cloudflare.com";

#[instrument(skip(ctx, doc), fields(trace_id))]
async fn reconcile(doc: Arc<Zone>, ctx: Arc<Context>) -> Result<Action> {
    let trace_id = telemetry::get_trace_id();
    if trace_id != opentelemetry::trace::TraceId::INVALID {
        Span::current().record("trace_id", field::display(&trace_id));
    }
    let _timer = ctx.metrics.reconcile.count_and_measure(&trace_id);
    ctx.diagnostics.write().await.last_event = Utc::now();
    let ns = doc.namespace().unwrap(); // doc is namespace scoped
    let docs: Api<Zone> = Api::namespaced(ctx.client.clone(), &ns);

    info!("Reconciling Zone \"{}\" in {}", doc.name_any(), ns);
    finalizer(&docs, DOCUMENT_FINALIZER, doc, |event| async {
        match event {
            Finalizer::Apply(doc) => doc.reconcile(ctx.clone()).await,
            Finalizer::Cleanup(doc) => doc.cleanup(ctx.clone()).await,
        }
    })
    .await
    .map_err(|e| Error::FinalizerError(Box::new(e)))
}

fn error_policy(doc: Arc<Zone>, error: &Error, ctx: Arc<Context>) -> Action {
    warn!("reconcile failed: {:?}", error);
    ctx.metrics.reconcile.set_failure_zone(&doc, error);
    Action::requeue(Duration::from_secs(5 * 60))
}

impl Zone {
    // Reconcile (for non-finalizer related changes)
    async fn reconcile(&self, ctx: Arc<Context>) -> Result<Action> {
        let client = ctx.client.clone();
        let ns = self.namespace().unwrap(); // we unwrap this, because it's probably impossible to
        // have no ns on the namespaced object
        let name = self.name_any();
        let docs: Api<Zone> = Api::namespaced(client, &ns);

        if name == "illegal" {
            return Err(Error::IllegalDocument); // error names show up in metrics
        }

        let zone_params = CreateZoneParams {
            name: &name,
            account: &self.spec.account,
            jump_start: None,
            zone_type: None,
        };
        // always overwrite status object with what we saw
        let res = ctx.cf_client.create_zone(zone_params).await;

        let _o = docs
            .patch_status(
                &name,
                &PatchParams::apply("cntrlr").force(),
                &Patch::Apply(json!({
                    "apiVersion": "cloudflare.com/v1alpha1",
                    "kind": "Zone",
                    "status": ZoneStatus {
                        ready: res.is_ok(),
                        zone_id: res?,
                    }
                })),
            )
            .await
            .map_err(Error::KubeError)?;

        // If no events were received, check back every 5 minutes
        Ok(Action::requeue(Duration::from_secs(5 * 60)))
    }

    // Finalizer cleanup (the object was deleted, ensure nothing is orphaned)
    async fn cleanup(&self, ctx: Arc<Context>) -> Result<Action> {
        let oref = self.object_ref(&());
        // Document doesn't have any real cleanup, so we just publish an event
        ctx.recorder
            .publish(
                &Event {
                    type_: EventType::Normal,
                    reason: "DeleteRequested".into(),
                    note: Some(format!("Delete `{}`", self.name_any())),
                    action: "Deleting".into(),
                    secondary: None,
                },
                &oref,
            )
            .await
            .map_err(Error::KubeError)?;
        Ok(Action::await_change())
    }
}

/// Initialize the controller and shared state (given the crd is installed)
pub async fn run(state: State) {
    let client = Client::try_default().await.expect("failed to create kube Client");
    let docs = Api::<Zone>::all(client.clone());
    if let Err(e) = docs.list(&ListParams::default().limit(1)).await {
        error!("CRD is not queryable; {e:?}. Is the CRD installed?");
        info!("Installation: cargo run --bin crdgen | kubectl apply -f -");
        std::process::exit(1);
    }

    let api_key =
        std::env::var("CLOUDFLARE_API_TOKEN").expect("CLOUDFLARE_API_TOKEN environment variable must be set");
    let cf_client = cf_client::CloudflareClient::new(api_key)
        .expect("Couldn't create cloudflare client")
        .into();
    Controller::new(docs, Config::default().any_semantic())
        .shutdown_on_signal()
        .run(reconcile, error_policy, state.to_context(client, cf_client).await)
        .filter_map(|x| async move { std::result::Result::ok(x) })
        .for_each(|_| futures::future::ready(()))
        .await;
}
