//! Prometheus metrics registry and collection helpers.
//!
//! Enabled only when the `metrics` Cargo feature is active.
//! Exposes a global [`Registry`] with the following instruments:
//!
//! | Name | Type | Labels | Description |
//! |---|---|---|---|
//! | `osp_requests_total` | Counter | method, bucket, status | Total requests |
//! | `osp_request_errors_total` | Counter | method, bucket, error | Total errors |
//! | `osp_active_connections` | Gauge | — | Current open connections |
//! | `osp_request_duration_seconds` | Histogram | method, bucket | Latency |
//! | `osp_transfer_bytes_total` | Counter | direction (rx/tx), bucket | Bytes transferred |
//! | `osp_presigned_url_hits_total` | Counter | bucket | Presigned URL hits |
//! | `osp_memory_bytes` | Gauge | — | RSS memory usage |
//! | `osp_build_info` | Gauge | version, rustc | Static build metadata |

use once_cell::sync::Lazy;
use prometheus::{
    Encoder, Gauge, GaugeVec, HistogramOpts, HistogramVec, IntCounterVec, IntGauge, Opts, Registry,
    TextEncoder,
};

/// The global Prometheus registry for all OSP metrics.
pub static REGISTRY: Lazy<Registry> = Lazy::new(Registry::new);

// ── Counters ──────────────────────────────────────────────────────────────────

/// Total number of proxied requests, labelled by HTTP method, bucket and
/// upstream response status code.
pub static REQUESTS_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let opts = Opts::new("osp_requests_total", "Total proxied requests").namespace("osp");
    let counter = IntCounterVec::new(opts, &["method", "bucket", "status"])
        .expect("osp_requests_total metric created");
    REGISTRY
        .register(Box::new(counter.clone()))
        .expect("register");
    counter
});

/// Total number of requests that resulted in an error (4xx/5xx or internal).
pub static REQUEST_ERRORS_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let opts = Opts::new("osp_request_errors_total", "Total request errors").namespace("osp");
    let counter = IntCounterVec::new(opts, &["method", "bucket", "error"])
        .expect("osp_request_errors_total metric created");
    REGISTRY
        .register(Box::new(counter.clone()))
        .expect("register");
    counter
});

/// Bytes transferred, labelled by direction (`rx` = client→proxy,
/// `tx` = proxy→client) and bucket.
pub static TRANSFER_BYTES_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let opts = Opts::new("osp_transfer_bytes_total", "Total bytes transferred").namespace("osp");
    let counter = IntCounterVec::new(opts, &["direction", "bucket"])
        .expect("osp_transfer_bytes_total metric created");
    REGISTRY
        .register(Box::new(counter.clone()))
        .expect("register");
    counter
});

/// Total presigned-URL hits per bucket.
pub static PRESIGNED_URL_HITS_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let opts = Opts::new(
        "osp_presigned_url_hits_total",
        "Total presigned URL requests",
    )
    .namespace("osp");
    let counter =
        IntCounterVec::new(opts, &["bucket"]).expect("osp_presigned_url_hits_total metric created");
    REGISTRY
        .register(Box::new(counter.clone()))
        .expect("register");
    counter
});

/// Total presigned-URL rejections (usage limit exceeded).
pub static PRESIGNED_URL_REJECTED_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let opts = Opts::new(
        "osp_presigned_url_rejected_total",
        "Presigned URL requests rejected due to usage limit",
    )
    .namespace("osp");
    let counter = IntCounterVec::new(opts, &["bucket"])
        .expect("osp_presigned_url_rejected_total metric created");
    REGISTRY
        .register(Box::new(counter.clone()))
        .expect("register");
    counter
});

// ── Gauges ────────────────────────────────────────────────────────────────────

/// Number of currently active (in-flight) connections.
pub static ACTIVE_CONNECTIONS: Lazy<IntGauge> = Lazy::new(|| {
    let opts = Opts::new("osp_active_connections", "Current active connections").namespace("osp");
    let gauge = IntGauge::with_opts(opts).expect("osp_active_connections metric created");
    REGISTRY
        .register(Box::new(gauge.clone()))
        .expect("register");
    gauge
});

/// Resident Set Size in bytes (sampled at scrape time via [`update_memory_gauge`]).
pub static MEMORY_BYTES: Lazy<Gauge> = Lazy::new(|| {
    let opts = Opts::new("osp_memory_bytes", "Resident set size in bytes").namespace("osp");
    let gauge = Gauge::with_opts(opts).expect("osp_memory_bytes metric created");
    REGISTRY
        .register(Box::new(gauge.clone()))
        .expect("register");
    gauge
});

/// Static build-info gauge — always 1, used to expose version labels.
pub static BUILD_INFO: Lazy<GaugeVec> = Lazy::new(|| {
    let opts = Opts::new("osp_build_info", "Static build metadata (always 1)").namespace("osp");
    let gauge = GaugeVec::new(opts, &["version", "rustc"]).expect("osp_build_info metric created");
    REGISTRY
        .register(Box::new(gauge.clone()))
        .expect("register");
    gauge
});

// ── Histograms ────────────────────────────────────────────────────────────────

/// End-to-end request latency in seconds, labelled by method and bucket.
pub static REQUEST_DURATION_SECONDS: Lazy<HistogramVec> = Lazy::new(|| {
    let buckets = vec![
        0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
    ];
    let opts = HistogramOpts::new(
        "osp_request_duration_seconds",
        "End-to-end request latency in seconds",
    )
    .namespace("osp")
    .buckets(buckets);
    let hist = HistogramVec::new(opts, &["method", "bucket"])
        .expect("osp_request_duration_seconds metric created");
    REGISTRY.register(Box::new(hist.clone())).expect("register");
    hist
});

/// Response body size histogram (bytes), labelled by method and bucket.
pub static RESPONSE_SIZE_BYTES: Lazy<HistogramVec> = Lazy::new(|| {
    let buckets = prometheus::exponential_buckets(1024.0, 4.0, 10).expect("valid bucket spec");
    let opts = HistogramOpts::new("osp_response_size_bytes", "Response body size in bytes")
        .namespace("osp")
        .buckets(buckets);
    let hist = HistogramVec::new(opts, &["method", "bucket"])
        .expect("osp_response_size_bytes metric created");
    REGISTRY.register(Box::new(hist.clone())).expect("register");
    hist
});

// ── Init ──────────────────────────────────────────────────────────────────────

/// Force all `Lazy` statics to initialise and record the build-info gauge.
///
/// Call this once from [`crate::run_server`] before the server starts accepting
/// connections.
pub fn init_metrics() {
    // Touch each lazy to ensure it is registered.
    Lazy::force(&REQUESTS_TOTAL);
    Lazy::force(&REQUEST_ERRORS_TOTAL);
    Lazy::force(&TRANSFER_BYTES_TOTAL);
    Lazy::force(&PRESIGNED_URL_HITS_TOTAL);
    Lazy::force(&PRESIGNED_URL_REJECTED_TOTAL);
    Lazy::force(&ACTIVE_CONNECTIONS);
    Lazy::force(&MEMORY_BYTES);
    Lazy::force(&BUILD_INFO);
    Lazy::force(&REQUEST_DURATION_SECONDS);
    Lazy::force(&RESPONSE_SIZE_BYTES);

    BUILD_INFO
        .with_label_values(&[env!("CARGO_PKG_VERSION"), "stable"])
        .set(1.0);
}

// ── Memory sampling ───────────────────────────────────────────────────────────

/// Read `/proc/self/status` on Linux to obtain RSS; no-op on other platforms.
pub fn update_memory_gauge() {
    #[cfg(target_os = "linux")]
    {
        if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
            for line in status.lines() {
                if line.starts_with("VmRSS:") {
                    if let Some(kb) = line
                        .split_whitespace()
                        .nth(1)
                        .and_then(|v| v.parse::<f64>().ok())
                    {
                        MEMORY_BYTES.set(kb * 1024.0);
                    }
                    break;
                }
            }
        }
    }
}

// ── Scrape endpoint ───────────────────────────────────────────────────────────

/// Encode the global registry to the Prometheus text format.
pub fn gather_metrics() -> String {
    update_memory_gauge();
    let encoder = TextEncoder::new();
    let mut buf = Vec::new();
    encoder
        .encode(&REGISTRY.gather(), &mut buf)
        .expect("metric encode");
    String::from_utf8(buf).unwrap_or_default()
}

/// Spawn a minimal Tokio HTTP server on `port` that serves `/metrics`.
///
/// The server runs in a background task and never blocks the Pingora thread.
pub async fn serve_metrics(port: u16) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind(format!("0.0.0.0:{}", port))
        .await
        .expect("failed to bind metrics port");

    tracing::info!(port, "Prometheus metrics endpoint listening");

    loop {
        let Ok((mut stream, _addr)) = listener.accept().await else {
            continue;
        };

        tokio::spawn(async move {
            let mut buf = [0u8; 4096];
            // Read just enough to identify the request path.
            let _ = stream.read(&mut buf).await;
            let req = String::from_utf8_lossy(&buf);

            let (status, body) = if req.contains("GET /metrics") {
                ("200 OK", gather_metrics())
            } else {
                ("404 Not Found", String::from("not found\n"))
            };

            let response = format!(
                "HTTP/1.1 {}\r\nContent-Type: text/plain; version=0.0.4\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                status,
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes()).await;
        });
    }
}
