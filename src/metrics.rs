use axum::{extract::State, response::IntoResponse};
use axum_prometheus::{metrics_exporter_prometheus::PrometheusHandle, PrometheusMetricLayer};
use metrics_process::Collector;
use std::time::Duration;

/// Initializes the Prometheus recorder and returns the metric layer + scrape handle.
/// Call once at startup before building the router.
pub fn init() -> (PrometheusMetricLayer<'static>, PrometheusHandle) {
    let (metric_layer, handle) = PrometheusMetricLayer::pair();
    (metric_layer, handle)
}

/// Spawns a background task that collects process metrics (memory, CPU, threads)
/// every 15 seconds using the metrics-process crate.
pub fn spawn_process_collector() {
    tokio::spawn(async move {
        let collector = Collector::default();
        let mut interval = tokio::time::interval(Duration::from_secs(15));
        loop {
            interval.tick().await;
            collector.collect();
        }
    });
}

/// Axum handler for GET /metrics — returns the Prometheus scrape text.
pub async fn metrics_handler(State(handle): State<PrometheusHandle>) -> impl IntoResponse {
    handle.render()
}
