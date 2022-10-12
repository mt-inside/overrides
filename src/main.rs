mod istio;

extern crate maplit;

use futures::prelude::*;
use istio::destinationrules_networking_istio_io::*;
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::{
    api::{Api, DynamicObject, ListParams, ObjectMeta, Patch, PatchParams, Resource, ResourceExt},
    runtime::{watcher, WatchStreamExt},
    Client, CustomResource,
};
use maplit::btreemap;
use tracing::*;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env()) // set env RUST_LOG="info|etc"
        //.with_max_level(Level::TRACE)
        .event_format(tracing_subscriber::fmt::format().pretty()) // pretty -> json
        .init();

    info!(event = "Connecting...");
    let client = Client::try_default().await?;
    info!(event = "Connected");

    let deps: Api<Deployment> = Api::default_namespaced(client.clone());
    // let drs: Api<DestinationRule> = Api::all(client.clone());
    // let vss: Api<VirtualService> = Api::all(client.clone());

    /* Debug print */
    for dep in deps.list(&Default::default()).await? {
        warn!(event = "Found Deploy", ?dep.metadata.name, ?dep.metadata.namespace);
    }
    // for dr in drs.list(&Default::default()).await? {
    //     debug!(event = "Found DR", ?dr.metadata.name, ?dr.metadata.namespace);
    // }
    // for vs in vss.list(&Default::default()).await? {
    //     debug!(event = "Found VS", ?vs.metadata.name, ?vs.metadata.namespace);
    // }

    let dr = DestinationRule {
        metadata: ObjectMeta {
            name: Some("foo".to_owned()),
            ..ObjectMeta::default()
        },
        spec: DestinationRuleSpec {
            host: Some("foo".to_owned()),
            subsets: Some(vec![DestinationRuleSubsets {
                name: Some("v1".to_owned()),
                labels: Some(btreemap![
                  "version".to_owned() => "v1".to_owned(),
                ]),
                ..DestinationRuleSubsets::default()
            }]),
            ..DestinationRuleSpec::default()
        },
    };

    println!("{:?}", dr);

    Ok(())
}
