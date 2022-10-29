use clap::Parser;
use k8s_openapi::api::core::v1::Service;
use kube::api::Api;
use overrides::istio::destinationrules_networking_istio_io::DestinationRule;
use overrides::istio::virtualservices_networking_istio_io::VirtualService;
use tracing::*;
use tracing_subscriber::{filter, prelude::*};

#[derive(Parser, Debug)]
#[command(name = env!("CARGO_BIN_NAME"))]
#[command(author = "Matt Turner")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "Generates Istio config for service-chain overrides", long_about = None)]
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
        .with(filter::Targets::new().with_target("overrides", Level::TRACE).with_target("override_generator", Level::TRACE)) //off|error|warn|info|debug|trace
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();

    let client = overrides::get_k8s_client().await?;

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
        let versions = overrides::svc_versions(&client, &svc).await?;
        debug!(
            service = svc.metadata.name,
            versions = ?versions,
            "Selects Pod versions",
        );
        let dr = overrides::dr_for_versions(&svc, &versions, None);
        let vs = overrides::vs_for_versions(&svc, &versions, None);
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
