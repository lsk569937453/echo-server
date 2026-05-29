use bytes::Bytes;
use clap::Parser;
use http_body_util::{combinators::BoxBody, BodyExt, Full};
use hyper::body::Incoming;
use hyper::header::HeaderValue;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::HeaderMap;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tokio_metrics::TaskMonitor;
use tracing_appender::rolling;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;
#[macro_use]
extern crate tracing;
#[derive(Parser)]
#[command(author, version, about, long_about)]
struct Cli {
    /// The http port,default port is 80
    #[arg(default_value_t = 80, short = 'P', long = "port", value_name = "Port")]
    http_port: u32,
    /// Maximum concurrent connections
    #[arg(default_value_t = 4096, short = 'C', long = "max-concurrency", value_name = "MaxConcurrency")]
    max_concurrency: usize,
}

fn convert_headers(headers: &HeaderMap<HeaderValue>) -> HashMap<String, String> {
    headers
        .iter()
        .map(|(k, v)| {
            (
                k.as_str().to_string(),
                String::from_utf8_lossy(v.as_bytes()).to_string(),
            )
        })
        .collect()
}

#[instrument(skip_all, fields(remote_addr = %addr))]
async fn echo(
    req: Request<Incoming>,
    addr: String,
) -> Result<Response<BoxBody<Bytes, hyper::Error>>, hyper::http::Error> {
    let uri = req.uri().clone();
    let path = uri.path();
    info!("Received request for path: {}", path);

    if path == "/api/delay" {
        // This is a long-blocking operation. In a real application,
        // you would want to handle this without blocking the executor thread.
        // For example, by spawning a blocking task.
        // tokio::task::spawn_blocking(move || {
        //     std::thread::sleep(Duration::from_secs(10));
        // }).await.unwrap();
        warn!("Simulating long delay for path: {}", path);
    }

    let headers = convert_headers(req.headers());
    let mut result_map = HashMap::new();
    result_map.insert("path".to_string(), path.to_string());
    result_map.insert("headers".to_string(), format!("{headers:?}"));

    let body_json = serde_json::to_string(&result_map).unwrap_or_default();

    Response::builder()
        .header("Content-Type", "application/json")
        .body(full(body_json))
}
async fn hello(
    _req: Request<Incoming>,
    addr: String,
) -> Result<Response<BoxBody<Bytes, hyper::Error>>, hyper::http::Error> {
    // info!("Hello handler called, remote_addr={}", addr);

    Response::builder()
        .header("Content-Type", "text/plain; charset=utf-8")
        .body(full("Hello World"))
}
fn full<T: Into<Bytes>>(chunk: T) -> BoxBody<Bytes, hyper::Error> {
    Full::new(chunk.into())
        .map_err(|never| match never {})
        .boxed()
}
fn setup_logger() -> Result<(), anyhow::Error> {
    let app_file = rolling::daily("./logs", "access.log");

    let file_layer = tracing_subscriber::fmt::Layer::new()
        .with_target(true)
        .with_ansi(false)
        .with_writer(app_file)
        .with_filter(tracing_subscriber::filter::LevelFilter::INFO);

    tracing_subscriber::registry()
        .with(file_layer)
        .with(tracing_subscriber::filter::LevelFilter::OFF)
        .init();
    Ok(())
}

// ── Application Metrics ────────────────────────────────────────────────

struct AppMetrics {
    /// inflight_requests: current number of requests being processed
    inflight_requests: AtomicU64,
    /// downstream latency (cumulative, microseconds)
    downstream_latency_us: AtomicU64,
    downstream_latency_count: AtomicU64,
    /// semaphore wait time (cumulative, microseconds)
    semaphore_wait_us: AtomicU64,
    semaphore_wait_count: AtomicU64,
    /// number of timed-out connections
    timeout_count: AtomicU64,
    /// connections waiting for a semaphore permit
    connection_pool_pending: AtomicU64,
    /// runtime queue latency: spawn → first-poll (cumulative, microseconds)
    runtime_queue_latency_us: AtomicU64,
    runtime_queue_latency_count: AtomicU64,
}

impl AppMetrics {
    fn new() -> Self {
        Self {
            inflight_requests: AtomicU64::new(0),
            downstream_latency_us: AtomicU64::new(0),
            downstream_latency_count: AtomicU64::new(0),
            semaphore_wait_us: AtomicU64::new(0),
            semaphore_wait_count: AtomicU64::new(0),
            timeout_count: AtomicU64::new(0),
            connection_pool_pending: AtomicU64::new(0),
            runtime_queue_latency_us: AtomicU64::new(0),
            runtime_queue_latency_count: AtomicU64::new(0),
        }
    }
}

/// Prometheus exposition handler
async fn metrics_handler(
    _req: Request<Incoming>,
    metrics: Arc<AppMetrics>,
) -> Result<Response<BoxBody<Bytes, hyper::Error>>, hyper::http::Error> {
    let inflight = metrics.inflight_requests.load(Ordering::Relaxed);
    let dl_sum = metrics.downstream_latency_us.load(Ordering::Relaxed);
    let dl_count = metrics.downstream_latency_count.load(Ordering::Relaxed);
    let dl_avg = if dl_count > 0 { dl_sum / dl_count } else { 0 };
    let sw_sum = metrics.semaphore_wait_us.load(Ordering::Relaxed);
    let sw_count = metrics.semaphore_wait_count.load(Ordering::Relaxed);
    let sw_avg = if sw_count > 0 { sw_sum / sw_count } else { 0 };
    let timeouts = metrics.timeout_count.load(Ordering::Relaxed);
    let pending = metrics.connection_pool_pending.load(Ordering::Relaxed);
    let rq_sum = metrics.runtime_queue_latency_us.load(Ordering::Relaxed);
    let rq_count = metrics.runtime_queue_latency_count.load(Ordering::Relaxed);
    let rq_avg = if rq_count > 0 { rq_sum / rq_count } else { 0 };

    let body = format!(
        "# HELP inflight_requests Number of requests currently being processed\n\
         # TYPE inflight_requests gauge\n\
         inflight_requests {inflight}\n\
         \n\
         # HELP downstream_latency_avg_us Average downstream response latency (us)\n\
         # TYPE downstream_latency_avg_us gauge\n\
         downstream_latency_avg_us {dl_avg}\n\
         # HELP downstream_latency_total_us Cumulative downstream response latency (us)\n\
         # TYPE downstream_latency_total_us counter\n\
         downstream_latency_total_us {dl_sum}\n\
         # HELP downstream_latency_count Total measured requests\n\
         # TYPE downstream_latency_count counter\n\
         downstream_latency_count {dl_count}\n\
         \n\
         # HELP semaphore_wait_avg_us Average semaphore acquisition wait time (us)\n\
         # TYPE semaphore_wait_avg_us gauge\n\
         semaphore_wait_avg_us {sw_avg}\n\
         # HELP semaphore_wait_total_us Cumulative semaphore wait time (us)\n\
         # TYPE semaphore_wait_total_us counter\n\
         semaphore_wait_total_us {sw_sum}\n\
         # HELP semaphore_wait_count Total semaphore acquisitions\n\
         # TYPE semaphore_wait_count counter\n\
         semaphore_wait_count {sw_count}\n\
         \n\
         # HELP timeout_count Total timed-out connections\n\
         # TYPE timeout_count counter\n\
         timeout_count {timeouts}\n\
         \n\
         # HELP connection_pool_pending Connections waiting for semaphore permit\n\
         # TYPE connection_pool_pending gauge\n\
         connection_pool_pending {pending}\n\
         \n\
         # HELP runtime_queue_latency_avg_us Average spawn-to-first-poll latency (us)\n\
         # TYPE runtime_queue_latency_avg_us gauge\n\
         runtime_queue_latency_avg_us {rq_avg}\n\
         # HELP runtime_queue_latency_total_us Cumulative spawn-to-first-poll latency (us)\n\
         # TYPE runtime_queue_latency_total_us counter\n\
         runtime_queue_latency_total_us {rq_sum}\n\
         # HELP runtime_queue_latency_count Total spawned tasks measured\n\
         # TYPE runtime_queue_latency_count counter\n\
         runtime_queue_latency_count {rq_count}\n"
    );

    Response::builder()
        .header("Content-Type", "text/plain; version=0.0.4; charset=utf-8")
        .body(full(body))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    setup_logger()?;
    let cli: Cli = Cli::parse();
    let port = cli.http_port;
    let addr = format!(r#"0.0.0.0:{port}"#);

    let listener = TcpListener::bind(&addr).await?;
    info!("Listening on http://{}", addr);
    println!("Listening on http://{addr}");

    let metrics = Arc::new(AppMetrics::new());
    let semaphore = Arc::new(Semaphore::new(cli.max_concurrency));

    // ── Background metrics reporter ────────────────────────────────────
    let bg_metrics = metrics.clone();
    let monitor = TaskMonitor::new();
    let runtime_handle = monitor.clone();

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        let mut prev_dl_us: u64 = 0;
        let mut prev_dl_count: u64 = 0;
        let mut prev_sw_us: u64 = 0;
        let mut prev_sw_count: u64 = 0;
        let mut prev_rq_us: u64 = 0;
        let mut prev_rq_count: u64 = 0;
        let mut prev_timeouts: u64 = 0;

        loop {
            interval.tick().await;
            let m = runtime_handle.cumulative();

            // --- delta: downstream latency ---
            let dl_us = bg_metrics.downstream_latency_us.load(Ordering::Relaxed);
            let dl_count = bg_metrics.downstream_latency_count.load(Ordering::Relaxed);
            let d_dl_us = dl_us.saturating_sub(prev_dl_us);
            let d_dl_n = dl_count.saturating_sub(prev_dl_count);
            let dl_avg = if d_dl_n > 0 { d_dl_us / d_dl_n } else { 0 };
            prev_dl_us = dl_us;
            prev_dl_count = dl_count;

            // --- delta: semaphore wait ---
            let sw_us = bg_metrics.semaphore_wait_us.load(Ordering::Relaxed);
            let sw_count = bg_metrics.semaphore_wait_count.load(Ordering::Relaxed);
            let d_sw_us = sw_us.saturating_sub(prev_sw_us);
            let d_sw_n = sw_count.saturating_sub(prev_sw_count);
            let sw_avg = if d_sw_n > 0 { d_sw_us / d_sw_n } else { 0 };
            prev_sw_us = sw_us;
            prev_sw_count = sw_count;

            // --- delta: runtime queue latency ---
            let rq_us = bg_metrics.runtime_queue_latency_us.load(Ordering::Relaxed);
            let rq_count = bg_metrics.runtime_queue_latency_count.load(Ordering::Relaxed);
            let d_rq_us = rq_us.saturating_sub(prev_rq_us);
            let d_rq_n = rq_count.saturating_sub(prev_rq_count);
            let rq_avg = if d_rq_n > 0 { d_rq_us / d_rq_n } else { 0 };
            prev_rq_us = rq_us;
            prev_rq_count = rq_count;

            // --- delta: timeout ---
            let timeouts = bg_metrics.timeout_count.load(Ordering::Relaxed);
            let d_timeouts = timeouts.saturating_sub(prev_timeouts);
            prev_timeouts = timeouts;

            // --- gauges ---
            let inflight = bg_metrics.inflight_requests.load(Ordering::Relaxed);
            let pending = bg_metrics.connection_pool_pending.load(Ordering::Relaxed);

            println!("========== TOKIO METRICS ==========");
            println!("Instrumented tasks: {}", m.instrumented_count);
            println!("Dropped tasks: {}", m.dropped_count);
            println!("First poll count: {}", m.first_poll_count);
            println!(
                "Total first poll delay: {:?}",
                m.total_first_poll_delay
            );
            println!("Total idled count: {}", m.total_idled_count);
            println!("Total scheduled count: {}", m.total_scheduled_count);
            println!("Total idle duration: {:?}", m.total_idle_duration);
            println!("---------- APP METRICS (5s) -------");
            println!("inflight_requests:       {inflight}");
            println!("downstream_latency_avg:  {dl_avg} us  ({d_dl_n} reqs)");
            println!("semaphore_wait_avg:      {sw_avg} us  ({d_sw_n} acqs)");
            println!("timeout_count:           +{d_timeouts}  (total {timeouts})");
            println!("connection_pool_pending: {pending}");
            println!("runtime_queue_latency:   {rq_avg} us  ({d_rq_n} tasks)");
            println!("===================================");
        }
    });

    // ── Accept loop ─────────────────────────────────────────────────────
    loop {
        let (stream, remote_addr) = listener.accept().await?;
        let addr_str = remote_addr.to_string();
        let io = TokioIo::new(stream);
        let metrics = metrics.clone();
        let semaphore = semaphore.clone();

        let spawn_time = Instant::now();

        tokio::spawn(async move {
            // runtime queue latency: spawn → first poll
            let queue_latency = spawn_time.elapsed();
            metrics.runtime_queue_latency_us.fetch_add(
                queue_latency.as_micros() as u64,
                Ordering::Relaxed,
            );
            metrics
                .runtime_queue_latency_count
                .fetch_add(1, Ordering::Relaxed);

            // Wait for semaphore permit
            metrics.connection_pool_pending.fetch_add(1, Ordering::Relaxed);
            let wait_start = Instant::now();

            let _permit = match tokio::time::timeout(
                Duration::from_secs(30),
                semaphore.acquire(),
            )
            .await
            {
                Ok(Ok(permit)) => {
                    let wait = wait_start.elapsed();
                    metrics
                        .semaphore_wait_us
                        .fetch_add(wait.as_micros() as u64, Ordering::Relaxed);
                    metrics.semaphore_wait_count.fetch_add(1, Ordering::Relaxed);
                    metrics.connection_pool_pending.fetch_sub(1, Ordering::Relaxed);
                    permit
                }
                _ => {
                    metrics.connection_pool_pending.fetch_sub(1, Ordering::Relaxed);
                    metrics.timeout_count.fetch_add(1, Ordering::Relaxed);
                    info!("Connection rejected (semaphore timeout), addr={addr_str}");
                    return;
                }
            };

            // Serve HTTP/1.1
            let addr_str_inner = addr_str.clone();
            let result = http1::Builder::new()
                .keep_alive(true)
                .serve_connection(
                    io,
                    service_fn(move |req: Request<Incoming>| {
                        let metrics = metrics.clone();
                        let addr = addr_str_inner.clone();
                        let path = req.uri().path().to_string();

                        async move {
                            let is_app_request = path != "/metrics";

                            if is_app_request {
                                metrics.inflight_requests.fetch_add(1, Ordering::Relaxed);
                            }

                            let start = Instant::now();

                            let result = match path.as_str() {
                                "/echo" => echo(req, addr).await,
                                "/metrics" => metrics_handler(req, metrics.clone()).await,
                                _ => hello(req, addr).await,
                            };

                            if is_app_request {
                                let elapsed = start.elapsed();
                                metrics.downstream_latency_us.fetch_add(
                                    elapsed.as_micros() as u64,
                                    Ordering::Relaxed,
                                );
                                metrics
                                    .downstream_latency_count
                                    .fetch_add(1, Ordering::Relaxed);
                                metrics.inflight_requests.fetch_sub(1, Ordering::Relaxed);
                            }

                            result
                        }
                    }),
                )
                .await;

            if let Err(err) = result {
                info!("Error serving connection: {:?},addr is:{:}", err, addr_str);
            }
        });
    }
}
