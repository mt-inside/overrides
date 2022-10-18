use anyhow::Result;
use futures::StreamExt;
use k8s_openapi::api::core::v1::Service;
use kube::{
    api::{Api, ListParams, Patch, PatchParams, Resource},
    runtime::controller::{Action, Controller},
    Client,
};
use overrides::istio::destinationrules_networking_istio_io::DestinationRule;
use overrides::istio::virtualservices_networking_istio_io::VirtualService;
use std::sync::Arc;
use thiserror::Error;
use tokio::time::Duration;
use tracing::*;
use tracing_subscriber::{filter, prelude::*};

#[derive(Debug, Error)]
enum Error {
    #[error("Failed to create resource: {0}")]
    ResourceCreationFailed(#[source] kube::Error),
    #[error("MissingObjectKey: {0}")]
    MissingObjectKey(&'static str),
    #[error("Failed to create resource: {0}")]
    GenerationFailed(#[source] overrides::Error),
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
    // Skip eg "kubernetes"
    if svc.spec.as_ref().unwrap().selector.is_none() {
        return Ok(Action::await_change());
    }

    let client = &ctx.client;

    let svc_ns = svc.metadata.namespace.as_ref().ok_or(Error::MissingObjectKey(".metadata.namespace"))?;
    let oref = svc.controller_owner_ref(&()).unwrap();

    let versions = overrides::svc_versions(client, &svc).await.map_err(Error::GenerationFailed)?;
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
    dr_api.patch(dr.metadata.name.as_ref().unwrap(), &PatchParams::apply("github.com/mt-inside/overrides"), &Patch::Apply(&dr)).await.map_err(Error::ResourceCreationFailed)?;
    vs_api.patch(vs.metadata.name.as_ref().unwrap(), &PatchParams::apply("github.com/mt-inside/overrides"), &Patch::Apply(&vs)).await.map_err(Error::ResourceCreationFailed)?;

    Ok(Action::requeue(Duration::from_secs(300)))
}

// The controller triggers this on reconcile errors
fn error_policy(_object: Arc<Service>, _error: &Error, _ctx: Arc<Data>) -> Action {
    Action::requeue(Duration::from_secs(1))
}
