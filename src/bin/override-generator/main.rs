use clap::Parser;
use k8s_openapi::api::core::v1::Service;
use kube::{api::Api, api::ObjectMeta};
use override_operator::istio::destinationrules_networking_istio_io::DestinationRule;
use override_operator::istio::virtualservices_networking_istio_io::VirtualService;
use tracing::*;
use tracing_subscriber::{filter, prelude::*};

#[derive(Parser, Debug)]
#[command(author = "Matt Turner", about = "Generates Istio config for service-chain overrides", version, long_about = None)]
struct Args {
    #[arg(short, long)]
    kubeconfig: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    if args.kubeconfig.is_some() {
        panic!("Don't support alternate kubeconfig location yet");
    };

    tracing_subscriber::registry()
        //.with(filter::EnvFilter::from_default_env()) // set env RUST_LOG="override_operator=off|error|warn|info|debug|trace"
        .with(filter::Targets::new().with_target("override_operator", Level::TRACE).with_target("override_generator", Level::TRACE)) //off|error|warn|info|debug|trace
        .with(
            tracing_subscriber::fmt::layer()
                .pretty()
                .with_file(false) // Don't print events' source file:line
                .with_writer(std::io::stderr),
        )
        .init();

    let client = override_operator::get_k8s_client().await?;

    let svcs_api: Api<Service> = Api::default_namespaced(client.clone());
    let mut drs: Vec<DestinationRule> = vec![];
    let mut vss: Vec<VirtualService> = vec![];
    for svc in svcs_api
        .list(&Default::default())
        .await?
        .into_iter()
        // Only services with selectors, eg not "kubernetes"
        .filter(|s| s.spec.as_ref().unwrap().selector.is_some())
    {
        let meta = ObjectMeta { name: svc.metadata.name.clone(), namespace: svc.metadata.namespace.clone(), ..ObjectMeta::default() };

        let versions = override_operator::svc_versions(&client, &svc).await?;
        info!(
            service = svc.metadata.name,
            versions = ?versions,
            "Selects Pod versions",
        );
        let dr = override_operator::dr_for_versions(&svc, &versions, meta.clone());
        let vs = override_operator::vs_for_versions(&svc, &versions, meta.clone());
        drs.push(dr);
        vss.push(vs);
    }

    for dr in drs {
        let dry = serde_yaml::to_string(&dr)?;
        println!("{}", dry);
        println!("---"); // hack
    }

    for vs in vss {
        let vsy = serde_yaml::to_string(&vs)?;
        println!("{}", vsy);
        println!("---"); // hack
    }

    Ok(())
}
