use crate::cli::Args;
use crate::stats::{ErrorKind, LiveStats, RequestCounter, WorkerStats};
use bytes::Bytes;
use http::uri::Uri;
use http_body_util::{BodyExt, Full};
use hyper_util::client::legacy::{connect::HttpConnector, Client};
use hyper_util::rt::TokioExecutor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::{sleep, interval};

type HttpsConnector = hyper_rustls::HttpsConnector<HttpConnector>;
type HyperClient = Client<HttpsConnector, Full<Bytes>>;

/// Configuration derived from CLI args, shared across workers.
#[derive(Clone)]
pub struct BenchConfig {
    pub uri: Uri,
    pub method: http::Method,
    pub headers: Vec<(String, String)>,
    pub body: Option<Bytes>,
    pub duration: Duration,
    pub timeout: Duration,
    pub http2: bool,
    pub rate_per_worker: u64,
}

impl BenchConfig {
    pub fn from_args(args: &Args, num_workers: usize) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let uri: Uri = args.url.parse()?;

        let method: http::Method = args.method.parse()?;

        let mut headers = Vec::new();
        for h in &args.headers {
            let (key, val) = h
                .split_once(':')
                .ok_or_else(|| format!("invalid header format: '{h}' (expected 'Key: Value')"))?;
            headers.push((key.trim().to_string(), val.trim().to_string()));
        }

        let body = if let Some(ref b) = args.body {
            Some(Bytes::from(b.clone()))
        } else if let Some(ref path) = args.body_file {
            Some(Bytes::from(std::fs::read(path)?))
        } else {
            None
        };

        let rate_per_worker = if args.rate > 0 {
            (args.rate / num_workers as u64).max(1)
        } else {
            0
        };

        Ok(Self {
            uri,
            method,
            headers,
            body,
            duration: args.duration,
            timeout: args.timeout,
            http2: args.http2,
            rate_per_worker,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_client_insecure() {
        let args = Args {
            url: "https://localhost".to_string(),
            connections: 1,
            requests: 0,
            duration: Duration::from_secs(1),
            threads: 1,
            method: "GET".to_string(),
            headers: vec![],
            body: None,
            body_file: None,
            http2: false,
            rate: 0,
            timeout: Duration::from_secs(5),
            insecure: true,
            latency: false,
        };
        let config = BenchConfig::from_args(&args, 1).unwrap();
        let _client = build_client(&config, true, 1);
    }
}

fn build_client(config: &BenchConfig, insecure: bool, num_connections: usize) -> HyperClient {
    let mut http = HttpConnector::new();
    http.set_nodelay(true);
    http.enforce_http(false);

    let tls = rustls::ClientConfig::builder();
    let tls_config = if insecure {
        tls.dangerous()
            .with_custom_certificate_verifier(Arc::new(NoVerifier))
            .with_no_client_auth()
    } else {
        tls.with_root_certificates(rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        })
        .with_no_client_auth()
    };

    let builder = hyper_rustls::HttpsConnectorBuilder::new()
        .with_tls_config(tls_config)
        .https_or_http();

    let https = if config.http2 {
        builder.enable_http2().build()
    } else {
        builder.enable_http1().build()
    };

    Client::builder(TokioExecutor::new())
        .pool_idle_timeout(Duration::from_secs(30))
        .pool_max_idle_per_host(num_connections)
        .build(https)
}

/// Runs the benchmark and returns per-worker stats.
pub async fn run(
    args: &Args,
) -> Result<(Vec<WorkerStats>, Duration), Box<dyn std::error::Error + Send + Sync>> {
    let num_workers = args.connections;
    let config = BenchConfig::from_args(args, num_workers)?;
    let counter = Arc::new(RequestCounter::new(args.requests));
    let live = Arc::new(LiveStats::new());
    let stop = Arc::new(AtomicBool::new(false));

    // Build a single shared client with a connection pool sized for all workers
    let client = build_client(&config, args.insecure, num_workers);

    // Spawn the timer task
    let stop_clone = stop.clone();
    let duration = config.duration;
    tokio::spawn(async move {
        sleep(duration).await;
        stop_clone.store(true, Ordering::Release);
    });

    let start = Instant::now();

    // Spawn progress reporter
    let live_clone = live.clone();
    let stop_clone = stop.clone();
    let progress_handle = tokio::spawn(async move {
        let mut tick = interval(Duration::from_millis(200));
        let mut prev_reqs: u64 = 0;
        let mut prev_time = start;
        loop {
            tick.tick().await;
            if stop_clone.load(Ordering::Acquire) {
                break;
            }
            let now = Instant::now();
            let elapsed = now.duration_since(start);
            let reqs = live_clone.requests.load(Ordering::Relaxed);
            let errs = live_clone.errors.load(Ordering::Relaxed);
            let bytes = live_clone.bytes.load(Ordering::Relaxed);

            let dt = now.duration_since(prev_time).as_secs_f64();
            let current_rps = if dt > 0.0 {
                (reqs - prev_reqs) as f64 / dt
            } else {
                0.0
            };
            prev_reqs = reqs;
            prev_time = now;

            crate::reporter::print_progress(elapsed, duration, reqs, errs, bytes, current_rps);
        }
    });

    // Spawn worker tasks — all sharing the same client
    let mut handles = Vec::with_capacity(num_workers);
    for _ in 0..num_workers {
        let config = config.clone();
        let counter = counter.clone();
        let live = live.clone();
        let stop = stop.clone();
        let client = client.clone();

        handles.push(tokio::spawn(async move {
            worker(client, &config, counter, &live, &stop).await
        }));
    }

    // Collect results
    let mut all_stats = Vec::with_capacity(num_workers);
    for handle in handles {
        all_stats.push(handle.await?);
    }

    // Stop progress reporter and clear the line
    stop.store(true, Ordering::Release);
    let _ = progress_handle.await;
    eprint!("\r\x1b[2K");

    let elapsed = start.elapsed();
    Ok((all_stats, elapsed))
}

async fn worker(
    client: HyperClient,
    config: &BenchConfig,
    counter: Arc<RequestCounter>,
    live: &LiveStats,
    stop: &AtomicBool,
) -> WorkerStats {
    let mut stats = WorkerStats::new();
    let rate_limited = config.rate_per_worker > 0;

    // If rate-limited, create an interval timer
    let mut rate_interval = if rate_limited {
        let period = Duration::from_secs_f64(1.0 / config.rate_per_worker as f64);
        Some(interval(period))
    } else {
        None
    };

    loop {
        // Check if we should stop
        if stop.load(Ordering::Acquire) || !counter.try_acquire() {
            break;
        }

        // Rate limiting
        if let Some(ref mut iv) = rate_interval {
            iv.tick().await;
        }

        // Build request
        let body = config
            .body
            .clone()
            .map(Full::new)
            .unwrap_or_else(|| Full::new(Bytes::new()));

        let mut builder = http::Request::builder()
            .method(config.method.clone())
            .uri(config.uri.clone());

        for (key, val) in &config.headers {
            builder = builder.header(key.as_str(), val.as_str());
        }

        let request = match builder.body(body) {
            Ok(r) => r,
            Err(_) => {
                stats.record_error(ErrorKind::Connect);
                continue;
            }
        };

        let req_start = Instant::now();

        // Send request with timeout
        let result = tokio::select! {
            res = send_request(&client, request) => res,
            _ = sleep(config.timeout) => Err(RequestError::Timeout),
        };

        let latency = req_start.elapsed();

        match result {
            Ok((status, body_len)) => {
                stats.record_latency(latency);
                stats.record_status(status);
                stats.bytes_received += body_len;
                live.requests.fetch_add(1, Ordering::Relaxed);
                live.bytes.fetch_add(body_len, Ordering::Relaxed);
            }
            Err(RequestError::Timeout) => {
                stats.record_error(ErrorKind::Timeout);
                live.requests.fetch_add(1, Ordering::Relaxed);
                live.errors.fetch_add(1, Ordering::Relaxed);
            }
            Err(RequestError::Connect(_)) => {
                stats.record_error(ErrorKind::Connect);
                live.requests.fetch_add(1, Ordering::Relaxed);
                live.errors.fetch_add(1, Ordering::Relaxed);
            }
            Err(RequestError::Read(_)) => {
                stats.record_error(ErrorKind::Read);
                live.requests.fetch_add(1, Ordering::Relaxed);
                live.errors.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    stats
}

#[derive(Debug)]
enum RequestError {
    Connect(String),
    Read(String),
    Timeout,
}

async fn send_request(
    client: &HyperClient,
    request: http::Request<Full<Bytes>>,
) -> Result<(u16, u64), RequestError> {
    let response = client
        .request(request)
        .await
        .map_err(|e| RequestError::Connect(e.to_string()))?;

    let status = response.status().as_u16();

    // Consume body to get accurate timing and byte count
    let body = response
        .into_body()
        .collect()
        .await
        .map_err(|e| RequestError::Read(e.to_string()))?;

    let bytes = body.to_bytes().len() as u64;
    Ok((status, bytes))
}

/// SECURITY: TLS certificate verifier that accepts everything (for --insecure flag).
///
/// This implementation completely bypasses TLS certificate and signature verification,
/// making the connection vulnerable to Man-in-the-Middle (MitM) attacks.
/// It should ONLY be used for testing with trusted endpoints and never in production
/// or over untrusted networks.
#[derive(Debug)]
struct NoVerifier;

impl rustls::client::danger::ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::ECDSA_NISTP521_SHA512,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
            rustls::SignatureScheme::ED25519,
            rustls::SignatureScheme::ED448,
        ]
    }
}
