use crate::{
    Context, Error, Result, State,
    cf_client::{self, CreateDnsRecordParams, DnsContent},
    dns_record::{DNSRecord, DNSRecordStatus},
    telemetry,
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
use std::{
    net::{Ipv4Addr, Ipv6Addr},
    sync::Arc,
};
use tokio::time::Duration;
use tracing::*;
pub static DOCUMENT_FINALIZER: &str = "dnsrecord.cloudflare.com";

#[instrument(skip(ctx, doc), fields(trace_id))]
async fn reconcile(doc: Arc<DNSRecord>, ctx: Arc<Context>) -> Result<Action> {
    let trace_id = telemetry::get_trace_id();
    if trace_id != opentelemetry::trace::TraceId::INVALID {
        Span::current().record("trace_id", field::display(&trace_id));
    }
    let _timer = ctx.metrics.reconcile.count_and_measure(&trace_id);
    ctx.diagnostics.write().await.last_event = Utc::now();
    let ns = doc.namespace().unwrap(); // doc is namespace scoped
    let docs: Api<DNSRecord> = Api::namespaced(ctx.client.clone(), &ns);

    info!("Reconciling DNSRecord \"{}\" in {}", doc.name_any(), ns);
    finalizer(&docs, DOCUMENT_FINALIZER, doc, |event| async {
        match event {
            Finalizer::Apply(doc) => doc.reconcile(ctx.clone()).await,
            Finalizer::Cleanup(doc) => doc.cleanup(ctx.clone()).await,
        }
    })
    .await
    .map_err(|e| Error::FinalizerError(Box::new(e)))
}

fn error_policy(doc: Arc<DNSRecord>, error: &Error, ctx: Arc<Context>) -> Action {
    warn!("reconcile failed: {:?}", error);
    ctx.metrics.reconcile.set_failure(&doc, error);
    Action::requeue(Duration::from_secs(5 * 60))
}

impl DNSRecord {
    // Reconcile (for non-finalizer related changes)
    async fn reconcile(&self, ctx: Arc<Context>) -> Result<Action> {
        let client = ctx.client.clone();
        let _oref = self.object_ref(&());
        let ns = self.namespace().unwrap(); // we unwrap this, because it's probably impossible to
        // have no ns on the namespaced object
        let name = self.name_any();
        let docs: Api<DNSRecord> = Api::namespaced(client, &ns);

        if name == "illegal" {
            return Err(Error::IllegalDocument); // error names show up in metrics
        }

        let _dns_rec: Api<DNSRecord> = Api::namespaced(ctx.client.clone(), &ns);

        let content = match self.spec.record_type.as_str() {
            "A" => DnsContent::A {
                content: self.spec.content.parse::<Ipv4Addr>()?,
            },
            "AAAA" => DnsContent::AAAA {
                content: self.spec.content.parse::<Ipv6Addr>()?,
            },
            "CNAME" => DnsContent::CNAME {
                content: self.spec.content.clone(),
            },
            "MX" => DnsContent::MX {
                content: self.spec.content.clone(),
                priority: self.spec.priority.unwrap_or(10),
            },
            "TXT" => DnsContent::TXT {
                content: self.spec.content.clone(),
            },
            _ => return Err(Error::UnsupportedRecordType(self.spec.record_type.clone())),
        };

        let dns_record_params = CreateDnsRecordParams {
            ttl: self.spec.ttl,
            priority: self.spec.priority,
            proxied: self.spec.proxied,
            name: self.spec.name.as_str(),
            content: content,
        };
        let res = ctx
            .cf_client
            .create_dns_record(self.spec.zone_id.as_str(), dns_record_params)
            .await?;
        // always overwrite status object with what we saw
        let new_status = Patch::Apply(json!({
            "apiVersion": "cloudflare.com/v1alpha1",
            "kind": "DNSRecord",
            "status": DNSRecordStatus {
                ready: true,
                record_id: Some(res),
            }
        }));
        let ps = PatchParams::apply("cntrlr").force();
        let _o = docs
            .patch_status(&name, &ps, &new_status)
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
    let docs = Api::<DNSRecord>::all(client.clone());
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

//TODO: reanimate tests
// Mock tests relying on fixtures.rs and its primitive apiserver mocks
/*#[cfg(test)]
mod test {
    use super::{Context, DNSRecord, error_policy, reconcile};
    use crate::{
        fixtures::{Scenario, timeout_after_1s},
        metrics::ErrorLabels,
    };
    use std::sync::Arc;

    #[tokio::test]
    async fn documents_without_finalizer_gets_a_finalizer() {
        let (testctx, fakeserver) = Context::test();
        let doc = DNSRecord::test();
        let mocksrv = fakeserver.run(Scenario::FinalizerCreation(doc.clone()));
        reconcile(Arc::new(doc), testctx).await.expect("reconciler");
        timeout_after_1s(mocksrv).await;
    }

    #[tokio::test]
    async fn finalized_doc_causes_status_patch() {
        let (testctx, fakeserver) = Context::test();
        let doc = DNSRecord::test().finalized();
        let mocksrv = fakeserver.run(Scenario::StatusPatch(doc.clone()));
        reconcile(Arc::new(doc), testctx).await.expect("reconciler");
        timeout_after_1s(mocksrv).await;
    }

    #[tokio::test]
    async fn finalized_doc_with_hide_causes_event_and_hide_patch() {
        let (testctx, fakeserver) = Context::test();
        let doc = DNSRecord::test().finalized();
        let scenario = Scenario::EventPublishThenStatusPatch("HideRequested".into(), doc.clone());
        let mocksrv = fakeserver.run(scenario);
        reconcile(Arc::new(doc), testctx).await.expect("reconciler");
        timeout_after_1s(mocksrv).await;
    }

    #[tokio::test]
    async fn finalized_doc_with_delete_timestamp_causes_delete() {
        let (testctx, fakeserver) = Context::test();
        let doc = DNSRecord::test().finalized().needs_delete();
        let mocksrv = fakeserver.run(Scenario::Cleanup("DeleteRequested".into(), doc.clone()));
        reconcile(Arc::new(doc), testctx).await.expect("reconciler");
        timeout_after_1s(mocksrv).await;
    }

    #[tokio::test]
    async fn illegal_doc_reconcile_errors_which_bumps_failure_metric() {
        let (testctx, fakeserver) = Context::test();
        let doc = Arc::new(DNSRecord::illegal().finalized());
        let mocksrv = fakeserver.run(Scenario::RadioSilence);
        let res = reconcile(doc.clone(), testctx.clone()).await;
        timeout_after_1s(mocksrv).await;
        assert!(res.is_err(), "apply reconciler fails on illegal doc");
        let err = res.unwrap_err();
        assert!(err.to_string().contains("IllegalDocument"));
        // calling error policy with the reconciler error should cause the correct metric to be set
        error_policy(doc.clone(), &err, testctx.clone());
        let err_labels = ErrorLabels {
            instance: "illegal".into(),
            error: "finalizererror(applyfailed(illegaldocument))".into(),
        };
        let metrics = &testctx.metrics.reconcile;
        let failures = metrics.failures.get_or_create(&err_labels).get();
        assert_eq!(failures, 1);
    }

    // Integration test without mocks
    //    use kube::api::{Api, ListParams, Patch, PatchParams};
    //    #[tokio::test]
    //    #[ignore = "uses k8s current-context"]
    //    async fn integration_reconcile_should_set_status_and_send_event() {
    //        let client = kube::Client::try_default().await.unwrap();
    //        let ctx = super::State::default().to_context(client.clone()).await;
    //
    //        // create a test doc
    //        let doc = DNSRecord::test().finalized();
    //        let docs: Api<DNSRecord> = Api::namespaced(client.clone(), "default");
    //        let ssapply = PatchParams::apply("ctrltest");
    //        let patch = Patch::Apply(doc.clone());
    //        docs.patch("test", &ssapply, &patch).await.unwrap();
    //
    //        // reconcile it (as if it was just applied to the cluster like this)
    //        reconcile(Arc::new(doc), ctx).await.unwrap();
    //
    //        // verify side-effects happened
    //        let output = docs.get_status("test").await.unwrap();
    //        assert!(output.status.is_some());
    //        // verify hide event was found
    //        let events: Api<k8s_openapi::api::core::v1::Event> = Api::all(client.clone());
    //        let opts = ListParams::default().fields("involvedObject.kind=Document,involvedObject.name=test");
    //        let event = events
    //            .list(&opts)
    //            .await
    //            .unwrap()
    //            .into_iter()
    //            .filter(|e| e.reason.as_deref() == Some("HideRequested"))
    //            .last()
    //            .unwrap();
    //        dbg!("got ev: {:?}", &event);
    //        assert_eq!(event.action.as_deref(), Some("Hiding"));
    //    }
}
*/
