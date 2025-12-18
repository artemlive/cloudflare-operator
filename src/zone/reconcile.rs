use crate::{
    Context, Error, Result, State,
    account::Account,
    cf_client::CreateZoneParams,
    telemetry,
    zone::{Zone, ZoneStatus},
};
use chrono::Utc;
use futures::StreamExt;
use kube::{
    Error as KubeError, Resource,
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
    ctx.metrics.reconcile.set_failure(doc.as_ref(), error);
    Action::requeue(Duration::from_secs(5 * 60))
}

impl Zone {
    // Reconcile (for non-finalizer related changes)
    async fn reconcile(&self, ctx: Arc<Context>) -> Result<Action> {
        let client = ctx.client.clone();
        let ns = self.namespace().unwrap(); // we unwrap this, because it's probably impossible to
        // have no ns on the namespaced object
        let name = self.name_any();
        let docs: Api<Zone> = Api::namespaced(client.clone(), &ns);
        let acc_api: Api<Account> = Api::namespaced(client, &ns);
        if let Some(a_ref) = &self.spec.account_ref {
            match acc_api.get(&a_ref.name).await {
                Ok(acc) => {
                    if let Some(a_status) = acc.status.as_ref()
                        && a_status.ready
                    {
                        let create_zone = CreateZoneParams {
                            name: &name,
                            account: &acc.spec.id,
                            jump_start: None,
                            zone_type: None,
                        };

                        match ctx
                            .provider
                            .get_client(self, &ns)
                            .await
                            .unwrap() // @FIXME: We need poscess it
                            .create_zone(create_zone)
                            .await
                        {
                            Ok(zone_id) => {
                                docs.patch_status(
                                    &name,
                                    &PatchParams::apply("cntrlr").force(),
                                    &Patch::Apply(json!({
                                        "apiVersion": "cloudflare.com/v1alpha1",
                                        "kind": "Zone",
                                        "status": ZoneStatus {
                                            ready: true,
                                            id: Some(zone_id),
                                            error: None,
                                        }
                                    })),
                                )
                                .await
                                .map_err(Error::KubeError)?;

                                return Ok(Action::requeue(Duration::from_secs(5 * 60)));
                            }
                            Err(e) => {
                                eprintln!("Error happend: {}", e);
                                docs.patch_status(
                                    &name,
                                    &PatchParams::apply("cntrlr").force(),
                                    &Patch::Apply(json!({
                                        "apiVersion": "cloudflare.com/v1alpha1",
                                        "kind": "Zone",
                                        "status": ZoneStatus {
                                            ready: false,
                                            id: None,
                                            error: Some(e.to_string()),
                                        }
                                    })),
                                )
                                .await
                                .map_err(Error::KubeError)?;
                                return Ok(Action::requeue(Duration::from_secs(60)));
                            }
                        }
                    } else {
                        docs.patch_status(
                            &name,
                            &PatchParams::apply("cntrlr").force(),
                            &Patch::Apply(json!({
                                "apiVersion": "cloudflare.com/v1alpha1",
                                "kind": "Zone",
                                "status": ZoneStatus {
                                    ready: false,
                                    id: None,
                                    error: Some(format!("Dependency account/{} is not ready", acc.name_any())),
                                }
                            })),
                        )
                        .await
                        .map_err(Error::KubeError)?;
                        return Ok(Action::requeue(Duration::from_secs(60)));
                    }
                }
                Err(KubeError::Api(e)) if e.code == 404 => {
                    eprintln!("Account '{}' not found in '{}' namespace", &a_ref.name, &ns);
                    docs.patch_status(
                        &name,
                        &PatchParams::apply("cntrlr").force(),
                        &Patch::Apply(json!({
                            "apiVersion": "cloudflare.com/v1alpha1",
                            "kind": "Zone",
                            "status": ZoneStatus {
                                ready: false,
                                id: None,
                                error: Some(format!("Dependency account/{} not found", a_ref.name)),
                            }
                        })),
                    )
                    .await
                    .map_err(Error::KubeError)?;

                    return Ok(Action::requeue(Duration::from_secs(30)));
                }
                Err(e) => {
                    return Err(Error::KubeError(e));
                }
            }
        }


        if name == "illegal" {
            return Err(Error::IllegalDocument); // error names show up in metrics
        }

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
    Controller::new(docs, Config::default().any_semantic())
        .shutdown_on_signal()
        .run(reconcile, error_policy, state.to_context(client, api_key).await)
        .filter_map(|x| async move { std::result::Result::ok(x) })
        .for_each(|_| futures::future::ready(()))
        .await;
}
