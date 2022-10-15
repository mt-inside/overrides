pub mod istio;

extern crate maplit;

use istio::destinationrules_networking_istio_io::*;
use istio::virtualservices_networking_istio_io::*;
use k8s_openapi::api::core::v1::{Pod, Service};
use kube::{
    api::{Api, ListParams, ObjectMeta},
    Client,
};
use maplit::btreemap;
use std::collections::BTreeMap;
use std::fmt;
use thiserror::Error;
use tracing::*;

struct Selector<'a>(&'a BTreeMap<String, String>);

impl fmt::Display for Selector<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.iter().map(|(k, v)| format!("{}={}", k, v)).collect::<Vec<String>>().join(","))
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("Failed to connect to cluster: {0}")]
    ConnectFailed(#[source] kube::Error),
    #[error("Failed to list resources: {0}")]
    ListResourcesFailed(#[source] kube::Error),
}

pub async fn get_k8s_client() -> Result<Client, Error> {
    debug!("Connecting...");
    let client = Client::try_default().await.map_err(Error::ConnectFailed)?;
    let ver = client.apiserver_version().await.map_err(Error::ConnectFailed)?;
    debug!(version = ver.git_version, platform = ver.platform, "Connected");

    Ok(client)
}

// TODO: factor into: svc2versinos, versions2dr, versions2vs
pub async fn svc_versions(client: &Client, svc: &Service) -> Result<Vec<String>, Error> {
    let pods_api: Api<Pod> = Api::default_namespaced(client.clone());

    trace!(svc.metadata.name, svc.metadata.namespace, "Found SVC");

    let selected_pods = pods_api
        .list(&ListParams::default().labels(
            &Selector(svc.spec.as_ref().unwrap().selector.as_ref().unwrap()).to_string(), // to_string invokes Display::fmt()
        ))
        .await
        .map_err(Error::ListResourcesFailed)?;

    for pod in &selected_pods {
        trace!(pod.metadata.name, pod.metadata.namespace, version = pod.metadata.labels.as_ref().unwrap().get("version").unwrap(), "Selected Pod",);
    }

    Ok(selected_pods.iter().map(|p| p.metadata.labels.as_ref().unwrap().get("version").unwrap().clone()).collect::<Vec<String>>())
}

pub fn dr_for_versions(svc: &Service, versions: &Vec<String>, meta: ObjectMeta) -> DestinationRule {
    let host_fqdn = format!(
        "{}.{}.svc.cluster.local", // TODO: better way to get this?
        svc.metadata.name.clone().unwrap(),
        svc.metadata.namespace.clone().unwrap(),
    );

    DestinationRule {
        metadata: meta,
        spec: DestinationRuleSpec {
            host: Some(host_fqdn.clone()),
            subsets: Some(
                versions
                    .iter()
                    .map(|v| DestinationRuleSubsets {
                        name: Some(v.clone()),
                        labels: Some(btreemap![
                          "version".to_owned() => v.clone(),
                        ]),
                        ..DestinationRuleSubsets::default()
                    })
                    .collect::<Vec<DestinationRuleSubsets>>(),
            ),
            ..DestinationRuleSpec::default()
        },
    }
}

pub fn vs_for_versions(svc: &Service, versions: &Vec<String>, meta: ObjectMeta) -> VirtualService {
    let host_fqdn = format!(
        "{}.{}.svc.cluster.local", // TODO: better way to get this?
        svc.metadata.name.clone().unwrap(),
        svc.metadata.namespace.clone().unwrap(),
    );

    VirtualService {
        metadata: meta,
        spec: VirtualServiceSpec {
            // gateways: implicity "mesh"
            hosts: Some(vec![host_fqdn.clone()]),
            http: Some(
                vec![
                    versions
                        .iter()
                        .map(|v| VirtualServiceHttp {
                            r#match: Some(vec![VirtualServiceHttpMatch {
                                headers: Some(btreemap![
                                             "x-override".to_owned() => VirtualServiceHttpMatchHeaders{
                                                 exact: Some(format!("{}:{}", svc.metadata.name.as_ref().unwrap(), v)),
                                                 prefix: None,
                                                 regex: None,
                                             },
                                ]),
                                ..VirtualServiceHttpMatch::default()
                            }]),
                            route: Some(vec![VirtualServiceHttpRoute {
                                destination: Some(VirtualServiceHttpRouteDestination { host: Some(host_fqdn.clone()), port: None, subset: Some(v.clone()) }),
                                ..VirtualServiceHttpRoute::default()
                            }]),
                            ..VirtualServiceHttp::default()
                        })
                        .collect::<Vec<VirtualServiceHttp>>(),
                    vec![
                        // Default route: to v1
                        VirtualServiceHttp {
                            route: Some(vec![VirtualServiceHttpRoute {
                                destination: Some(VirtualServiceHttpRouteDestination { host: Some(host_fqdn.clone()), port: None, subset: Some("v1".to_owned()) }),
                                ..VirtualServiceHttpRoute::default()
                            }]),
                            ..VirtualServiceHttp::default()
                        },
                    ],
                ]
                .concat(),
            ),
            ..VirtualServiceSpec::default()
        },
    }
}
