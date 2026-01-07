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
use std::time::Duration;
use tokio::net::TcpListener;
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
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    setup_logger()?;
    let cli: Cli = Cli::parse();
    let port = cli.http_port;
    let addr = format!(r#"0.0.0.0:{port}"#);

    let listener = TcpListener::bind(&addr).await?;
    info!("Listening on http://{}", addr);
    println!("Listening on http://{addr}");
    let monitor = TaskMonitor::new();
    let runtime_handle = monitor.clone();

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        loop {
            interval.tick().await;
            let metrics = runtime_handle.cumulative();
            println!("========== TOKIO METRICS ==========");
            println!("Instrumented tasks: {}", metrics.instrumented_count);
            println!("Dropped tasks: {}", metrics.dropped_count);
            println!("First poll count: {}", metrics.first_poll_count);
            println!(
                "Total first poll delay: {:?}",
                metrics.total_first_poll_delay
            );
            println!("Total idled count: {}", metrics.total_idled_count);
            println!("Total scheduled count: {}", metrics.total_scheduled_count);
            println!("Total idle duration: {:?}", metrics.total_idle_duration);
            println!("===================================");
        }
    });

    loop {
        let (stream, addr) = listener.accept().await?;
        let addr_str = addr.to_string();
        let io = TokioIo::new(stream);
        tokio::spawn(async move {
            let addr_str_cloned = addr_str.clone();
            if let Err(err) = http1::Builder::new()
                .keep_alive(true)
                .serve_connection(
                    io,
                    service_fn(move |req: Request<Incoming>| {
                        let addr = addr_str_cloned.clone();
                        let path = req.uri().path().to_string();

                        async move {
                            match path.as_str() {
                                "/echo" => echo(req, addr).await,
                                _ => hello(req, addr).await,
                            }
                        }
                    }),
                )
                .await
            {
                info!("Error serving connection: {:?},addr is:{:}", err, addr_str);
            }
        });
    }
}
