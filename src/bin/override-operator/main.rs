mod metrics;

use actix_web::{middleware, web::Data, App, HttpServer};
use anyhow::Result;
use clap::Parser;
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
use tokio::time::{Duration, Instant};
use tracing::*;
use tracing_subscriber::{filter, prelude::*};

static SERVICE_FINALIZER_NAME: &str = "overrides.mt165.co.uk/Service";

static NAME: &str = env!("CARGO_BIN_NAME");
static VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser, Debug)]
#[command(name = NAME)]
#[command(author = "Matt Turner")]
#[command(version = VERSION)]
#[command(about = "Generates Istio config for service-chain overrides", long_about = None)]
struct Args {
    #[arg(short, long)]
    kubeconfig: Option<String>,
}

#[derive(Debug, Error)]
enum Error {
    #[error("MissingObjectKey: {0}")]
    MissingObjectKey(&'static str),
    #[error("Finalizer Error: {0}")]
    FinalizerError(#[source] kube::runtime::finalizer::Error<kube::Error>),
}

// Data we want access to in error/reconcile calls
struct ControllerCtx {
    client: Client,
    metrics: metrics::Metrics,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    if args.kubeconfig.is_some() {
        panic!("Don't support alternate kubeconfig location yet");
    };

    tracing_subscriber::registry()
        .with(filter::Targets::new().with_target("overrides", Level::TRACE).with_target("override_operator", Level::TRACE).with_target("actix_web", Level::DEBUG)) //off|error|warn|info|debug|trace
        .with(
            tracing_subscriber::fmt::layer()
                .pretty()
                .with_file(false) // Don't print events' source file:line
                .with_writer(std::io::stderr),
        )
        .init();

    info!(VERSION, "{}", NAME);

    let http_server = HttpServer::new(move || App::new().app_data(Data::new(())).wrap(middleware::Logger::default().exclude("/health")).service(metrics::metrics))
        .bind("0.0.0.0:8080")
        .expect("Can't bind to ::8080")
        .shutdown_timeout(5);

    let client = overrides::get_k8s_client().await?;

    let svc_api: Api<Service> = Api::default_namespaced(client.clone());
    let dr_api: Api<DestinationRule> = Api::default_namespaced(client.clone());
    let vs_api: Api<VirtualService> = Api::default_namespaced(client.clone());

    let controller = Controller::new(svc_api, ListParams::default())
        .owns(dr_api, ListParams::default())
        .owns(vs_api, ListParams::default())
        .shutdown_on_signal()
        .run(reconcile, error_policy, Arc::new(ControllerCtx { client, metrics: metrics::Metrics::new() }))
        .for_each(|res| async move {
            match res {
                Ok(o) => info!("reconciled {:?}", o),
                Err(e) => warn!("reconcile failed: {:?}", e),
            }
        });

    tokio::select! {
        _ = controller => warn!("Controller bailed"),
        _ = http_server.run() => warn!("Web server bailed"),
    };

    info!("terminiated");
    Ok(())
}

async fn reconcile(svc: Arc<Service>, ctx: Arc<ControllerCtx>) -> Result<Action, Error> {
    ctx.metrics.reconciliations.inc();
    let start_time = Instant::now();

    let client = &ctx.client;
    let svc_ns = svc.metadata.namespace.clone().ok_or(Error::MissingObjectKey(".metadata.namespace"))?;
    let svc_api: Api<Service> = Api::namespaced(client.clone(), &svc_ns);
    // finalizer()
    // * wraps the reconcile function (Evt::Apply)
    // * adds a finalizer ref to the applied objects
    // * impls the finalizer, which does deletion of owned objects for you
    //   * then calls Evt::Cleanup which is just for you to log or whatever
    let action = finalizer(&svc_api, SERVICE_FINALIZER_NAME, svc, |event| async {
        match event {
            // TODO: to member functions
            FinalizerEvt::Apply(svc) => update(svc, ctx.clone(), &svc_ns).await,
            FinalizerEvt::Cleanup(svc) => delete(svc, ctx.clone(), &svc_ns).await,
        }
    })
    .await
    .map_err(Error::FinalizerError);

    let duration = start_time.elapsed().as_millis() as f64 / 1000.0;
    ctx.metrics.reconcile_durations.with_label_values(&[]).observe(duration);

    action
}

async fn update(svc: Arc<Service>, ctx: Arc<ControllerCtx>, svc_ns: &str) -> Result<Action, kube::Error> {
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

async fn delete(svc: Arc<Service>, _ctx: Arc<ControllerCtx>, _svc_ns: &str) -> Result<Action, kube::Error> {
    info!(service = svc.metadata.name, "DestinationRule and VirtualService deleted by finalizer",);

    Ok(Action::await_change())
}

// The controller triggers this on reconcile errors
fn error_policy(_object: Arc<Service>, _error: &Error, ctx: Arc<ControllerCtx>) -> Action {
    ctx.metrics.failures.inc();
    Action::requeue(Duration::from_secs(1))
}
