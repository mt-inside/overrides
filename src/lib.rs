mod istio;

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
use tracing::*;

struct Selector<'a>(&'a BTreeMap<String, String>);

impl fmt::Display for Selector<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.iter().map(|(k, v)| format!("{}={}", k, v)).collect::<Vec<String>>().join(","))
    }
}

// TODO: anyhow -> thiserror
pub async fn generate(client: Client) -> anyhow::Result<(Vec<DestinationRule>, Vec<VirtualService>)> {
    let pods_api: Api<Pod> = Api::default_namespaced(client.clone());
    let svcs_api: Api<Service> = Api::default_namespaced(client.clone());
    // let drs: Api<DestinationRule> = Api::all(client.clone());
    // let vss: Api<VirtualService> = Api::all(client.clone());

    let mut drs: Vec<DestinationRule> = vec![];
    let mut vss: Vec<VirtualService> = vec![];

    for svc in svcs_api
        .list(&Default::default())
        .await?
        .into_iter()
        // Only services with selectors, eg not "kubernetes"
        .filter(|s| s.spec.as_ref().unwrap().selector.is_some())
    {
        let fqdn = format!(
            "{}.{}.svc.cluster.local", // TODO: better way to get this?
            svc.metadata.name.clone().unwrap(),
            svc.metadata.namespace.clone().unwrap(),
        );
        trace!(svc.metadata.name, svc.metadata.namespace, fqdn, "Found SVC");

        let selected_pods = pods_api
            .list(&ListParams::default().labels(
                &Selector(svc.spec.as_ref().unwrap().selector.as_ref().unwrap()).to_string(), // to_string invokes Display::fmt()
            ))
            .await?;

        for pod in &selected_pods {
            trace!(pod.metadata.name, pod.metadata.namespace, version = pod.metadata.labels.as_ref().unwrap().get("version").unwrap(), "Selected Pod",);
        }

        let selected_pod_versions: Vec<String> = selected_pods.iter().map(|p| p.metadata.labels.as_ref().unwrap().get("version").unwrap().clone()).collect();

        info!(
            service = svc.metadata.name,
            versions = ?selected_pod_versions,
            "Selects Pod versions",
        );

        let dr = DestinationRule {
            metadata: ObjectMeta { name: svc.metadata.name.clone(), namespace: svc.metadata.namespace.clone(), ..ObjectMeta::default() },
            spec: DestinationRuleSpec {
                host: Some(fqdn.clone()),
                subsets: Some(
                    selected_pod_versions
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
        };
        drs.push(dr);

        let vs = VirtualService {
            metadata: ObjectMeta { name: svc.metadata.name.clone().map(|n| format!("{}-overrides", n)), namespace: svc.metadata.namespace.clone(), ..ObjectMeta::default() },
            spec: VirtualServiceSpec {
                // gateways: implicity "mesh"
                hosts: Some(vec![fqdn.clone()]),
                http: Some(
                    vec![
                        selected_pod_versions
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
                                    destination: Some(VirtualServiceHttpRouteDestination { host: Some(fqdn.clone()), port: None, subset: Some(v.clone()) }),
                                    ..VirtualServiceHttpRoute::default()
                                }]),
                                ..VirtualServiceHttp::default()
                            })
                            .collect::<Vec<VirtualServiceHttp>>(),
                        vec![
                            // Default route: to v1
                            VirtualServiceHttp {
                                route: Some(vec![VirtualServiceHttpRoute {
                                    destination: Some(VirtualServiceHttpRouteDestination { host: Some(fqdn.clone()), port: None, subset: Some("v1".to_owned()) }),
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
        };
        vss.push(vs);
    }

    Ok((drs, vss))
}
