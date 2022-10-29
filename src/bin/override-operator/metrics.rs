use actix_web::{get, HttpRequest, HttpResponse, Responder};
use prometheus::{default_registry, register_histogram_vec, register_int_counter, Encoder, HistogramVec, IntCounter, TextEncoder};

pub struct Metrics {
    pub reconciliations: IntCounter,
    pub failures: IntCounter,
    pub reconcile_durations: HistogramVec,
}

impl Metrics {
    pub fn new() -> Self {
        // TODO: tempate in override_operator
        Metrics {
            reconciliations: register_int_counter!(format!("{}_reconciliations_total", crate::NAME.replace("-", "_")), "reconciliations").unwrap(),
            failures: register_int_counter!("override_operator_failures_total", "reconciliation failures").unwrap(),
            reconcile_durations: register_histogram_vec!("override_operator_reconcile_duration_seconds", "Duration of reconciles in seconds", &[], vec![0.01, 0.1, 0.25, 0.5, 1., 5., 15., 60.],)
                .unwrap(),
        }
    }
}

#[get("/metrics")]
async fn metrics(_data: actix_web::web::Data<()>, _req: HttpRequest) -> impl Responder {
    let encoder = TextEncoder::new();
    let mut buffer = vec![];
    encoder.encode(&default_registry().gather(), &mut buffer).unwrap();
    HttpResponse::Ok().body(buffer)
}
