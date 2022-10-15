use clap::Parser;
use kube::Client;
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

    //tracing_subscriber::fmt()
    //    .with_env_filter(EnvFilter::from_default_env()) // set env RUST_LOG="override_operator=off|error|warn|info|debug|trace"
    //    //.with_max_level(Level::TRACE)
    //    .event_format(tracing_subscriber::fmt::format().pretty()) // pretty -> json
    //    .init();

    tracing_subscriber::registry()
        .with(filter::Targets::new().with_target("override_operator", Level::TRACE)) //off|error|warn|info|debug|trace
        .with(
            tracing_subscriber::fmt::layer()
                .pretty()
                .with_file(false) // Don't print events' source file:line
                .with_writer(std::io::stderr),
        )
        .init();

    debug!("Connecting...");
    let client = Client::try_default().await?;
    let ver = client.apiserver_version().await?;
    debug!(version = ver.git_version, platform = ver.platform, "Connected");

    let (drs, vss) = override_operator::generate(client).await?;

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

    // for dr in drs.list(&Default::default()).await? {
    //     debug!(event = "Found DR", ?dr.metadata.name, ?dr.metadata.namespace);
    // }
    // for vs in vss.list(&Default::default()).await? {
    //     debug!(event = "Found VS", ?vs.metadata.name, ?vs.metadata.namespace);
    // }

    // TODO:
    // - separate binary entry points
    // - operator
    // - build into container and run in cluster

    Ok(())
}
