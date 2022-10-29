mod metrics;
mod operator;

#[macro_use]
extern crate maplit;

use actix_web::{get, middleware, web::Data, App, HttpRequest, HttpResponse, HttpServer, Responder};
use clap::Parser;
use tracing::*;
use tracing_subscriber::{filter, prelude::*};

pub static NAME: &str = env!("CARGO_BIN_NAME"); // has hypens; CARGO_CRATE_NAME for underscores
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

#[get("/healthz")]
async fn health(_data: actix_web::web::Data<()>, _req: HttpRequest) -> impl Responder {
    HttpResponse::Ok().json(hashmap!["health"=> "ok", "name" => NAME, "version" => VERSION])
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    if args.kubeconfig.is_some() {
        panic!("Don't support alternate kubeconfig location yet");
    };

    tracing_subscriber::registry()
        .with(
            filter::Targets::new()
                .with_default(Level::INFO)
                .with_target("overrides", Level::TRACE)
                .with_target("override_operator", Level::TRACE)
                .with_target("actix_server", Level::INFO)
                .with_target("actix_web", Level::DEBUG),
        ) //off|error|warn|info|debug|trace
        .with(
            tracing_subscriber::fmt::layer()
                //.pretty() - actually ugly
                //.with_file(false) // Don't print events' source file:line
                .with_writer(std::io::stderr),
        )
        .init();

    info!(VERSION, "{}", NAME);

    let http_server = HttpServer::new(move || App::new().app_data(Data::new(())).wrap(middleware::Logger::default().exclude("/health")).service(metrics::metrics).service(health))
        .bind("0.0.0.0:8080")
        .expect("Can't bind to ::8080")
        .shutdown_timeout(5);

    // get_controller() is async, and returns Result<Future<()>> - the Future is the promise to run the controller loop, the Result represents the fact it might no be possile to start the controller
    let controller = operator::get_controller().await?;

    // I guess select! is clever enough to poll the Future and spawn a task to block for the immediate function
    tokio::select! {
        // impl Future<()>
        _ = controller => info!("Controller finished"),
        // Result<(), std::io::Error>
        h = http_server.run() => match h {
            Err(err) => warn!(%err, "Web server error"),
            Ok(_) => info!("Web server finished"),
        },
    };

    info!("Quitting");
    Ok(())
}
