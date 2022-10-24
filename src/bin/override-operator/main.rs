use anyhow::Result;
use futures::StreamExt;
use k8s_openapi::api::core::v1::Service;
use kube::{
    api::{Api, ListParams, Patch, PatchParams, Resource},
    runtime::{
        controller::{Action, Controller},
        finalizer::{finalizer, Event as FinalizerEvt},
    },
    Client,
};
use overrides::istio::destinationrules_networking_istio_io::DestinationRule;
use overrides::istio::virtualservices_networking_istio_io::VirtualService;
use std::sync::Arc;
use thiserror::Error;
use tokio::time::Duration;
use tracing::*;
use tracing_subscriber::{filter, prelude::*};

static SERVICE_FINALIZER_NAME: &str = "overrides.mt165.co.uk/Service";

#[derive(Debug, Error)]
enum Error {
    #[error("MissingObjectKey: {0}")]
    MissingObjectKey(&'static str),
    #[error("Finalizer Error: {0}")]
    FinalizerError(#[source] kube::runtime::finalizer::Error<kube::Error>),
}

// Data we want access to in error/reconcile calls
struct Data {
    client: Client,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(filter::Targets::new().with_target("overrides", Level::TRACE).with_target("override_operator", Level::TRACE)) //off|error|warn|info|debug|trace
        .with(
            tracing_subscriber::fmt::layer()
                .pretty()
                .with_file(false) // Don't print events' source file:line
                .with_writer(std::io::stderr),
        )
        .init();

    let client = overrides::get_k8s_client().await?;

    let svc_api: Api<Service> = Api::default_namespaced(client.clone());
    let dr_api: Api<DestinationRule> = Api::default_namespaced(client.clone());
    let vs_api: Api<VirtualService> = Api::default_namespaced(client.clone());

    Controller::new(svc_api, ListParams::default())
        .owns(dr_api, ListParams::default())
        .owns(vs_api, ListParams::default())
        .shutdown_on_signal()
        .run(reconcile, error_policy, Arc::new(Data { client }))
        .for_each(|res| async move {
            match res {
                Ok(o) => info!("reconciled {:?}", o),
                Err(e) => warn!("reconcile failed: {:?}", e),
            }
        })
        .await;

    info!("controller terminiated");
    Ok(())
}

async fn reconcile(svc: Arc<Service>, ctx: Arc<Data>) -> Result<Action, Error> {
    let client = &ctx.client;
    let svc_ns = svc.metadata.namespace.clone().ok_or(Error::MissingObjectKey(".metadata.namespace"))?;
    let svc_api: Api<Service> = Api::namespaced(client.clone(), &svc_ns);
    // finalizer()
    // * wraps the reconcile function (Evt::Apply)
    // * adds a finalizer ref to the applied objects
    // * impls the finalizer, which does deletion of owned objects for you
    //   * then calls Evt::Cleanup which is just for you to log or whatever
    finalizer(&svc_api, SERVICE_FINALIZER_NAME, svc, |event| async {
        match event {
            // TODO: to member functions
            FinalizerEvt::Apply(svc) => update(svc, ctx.clone(), &svc_ns).await,
            FinalizerEvt::Cleanup(svc) => delete(svc, ctx.clone(), &svc_ns).await,
        }
    })
    .await
    .map_err(Error::FinalizerError)
}

async fn update(svc: Arc<Service>, ctx: Arc<Data>, svc_ns: &str) -> Result<Action, kube::Error> {
    // Skip eg "kubernetes"
    if svc.spec.as_ref().unwrap().selector.is_none() {
        return Ok(Action::await_change());
    }

    let client = &ctx.client;

    let oref = svc.controller_owner_ref(&()).unwrap();

    let versions = overrides::svc_versions(client, &svc).await?;
    info!(
        service = svc.metadata.name,
        versions = ?versions,
        "Selects Pod versions",
    );
    let dr = overrides::dr_for_versions(&svc, &versions, Some(oref.clone()));
    let vs = overrides::vs_for_versions(&svc, &versions, Some(oref.clone()));

    let dr_api: Api<DestinationRule> = Api::namespaced(client.clone(), svc_ns);
    let vs_api: Api<VirtualService> = Api::namespaced(client.clone(), svc_ns);

    // Server-side apply
    dr_api.patch(dr.metadata.name.as_ref().unwrap(), &PatchParams::apply("github.com/mt-inside/overrides"), &Patch::Apply(&dr)).await?;
    vs_api.patch(vs.metadata.name.as_ref().unwrap(), &PatchParams::apply("github.com/mt-inside/overrides"), &Patch::Apply(&vs)).await?;

    Ok(Action::requeue(Duration::from_secs(300)))
}

async fn delete(svc: Arc<Service>, _ctx: Arc<Data>, _svc_ns: &str) -> Result<Action, kube::Error> {
    info!(service = svc.metadata.name, "DestinationRule and VirtualService deleted by finalizer",);

    Ok(Action::await_change())
}

// The controller triggers this on reconcile errors
fn error_policy(_object: Arc<Service>, _error: &Error, _ctx: Arc<Data>) -> Action {
    Action::requeue(Duration::from_secs(1))
}
