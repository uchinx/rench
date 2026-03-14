use clap::Parser;
use std::time::Duration;

#[derive(Parser, Debug, Clone)]
#[command(
    name = "rench",
    about = "⚡ A high-performance HTTP benchmarking tool",
    version,
    long_about = "rench - High-performance HTTP load testing built with Rust.\n\n\
    Uses async I/O with tokio + hyper for maximum throughput.\n\
    Supports HTTP/1.1 and HTTP/2, TLS, custom headers, and request bodies."
)]
pub struct Args {
    /// Target URL to benchmark
    pub url: String,

    /// Number of concurrent connections
    #[arg(short = 'c', long, default_value_t = 50)]
    pub connections: usize,

    /// Total number of requests (0 = unlimited, use with --duration)
    #[arg(short = 'n', long, default_value_t = 0)]
    pub requests: u64,

    /// Test duration (e.g. 10s, 30s, 1m)
    #[arg(short = 'd', long, default_value = "10s", value_parser = parse_duration)]
    pub duration: Duration,

    /// Number of worker threads (defaults to CPU count)
    #[arg(short = 't', long, default_value_t = 0)]
    pub threads: usize,

    /// HTTP method
    #[arg(short = 'm', long, default_value = "GET")]
    pub method: String,

    /// Request headers (repeatable: -H "Key: Value")
    #[arg(short = 'H', long = "header", num_args = 1)]
    pub headers: Vec<String>,

    /// Request body
    #[arg(short = 'b', long)]
    pub body: Option<String>,

    /// Read request body from file
    #[arg(short = 'B', long = "body-file")]
    pub body_file: Option<String>,

    /// HTTP/2 only
    #[arg(long)]
    pub http2: bool,

    /// Target requests per second (0 = unlimited)
    #[arg(long, default_value_t = 0)]
    pub rate: u64,

    /// Connection timeout
    #[arg(long, default_value = "5s", value_parser = parse_duration)]
    pub timeout: Duration,

    /// Disable TLS certificate verification
    #[arg(short = 'k', long)]
    pub insecure: bool,

    /// Print latency distribution at given percentiles
    #[arg(long)]
    pub latency: bool,
}

fn parse_duration(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    if let Some(val) = s.strip_suffix("ms") {
        val.parse::<u64>()
            .map(Duration::from_millis)
            .map_err(|e| e.to_string())
    } else if let Some(val) = s.strip_suffix('s') {
        val.parse::<u64>()
            .map(Duration::from_secs)
            .map_err(|e| e.to_string())
    } else if let Some(val) = s.strip_suffix('m') {
        val.parse::<u64>()
            .map(|v| Duration::from_secs(v * 60))
            .map_err(|e| e.to_string())
    } else {
        s.parse::<u64>()
            .map(Duration::from_secs)
            .map_err(|e| format!("invalid duration '{}': {}", s, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_duration() {
        // Success cases
        assert_eq!(parse_duration("100ms"), Ok(Duration::from_millis(100)));
        assert_eq!(parse_duration("10s"), Ok(Duration::from_secs(10)));
        assert_eq!(parse_duration("2m"), Ok(Duration::from_secs(120)));
        assert_eq!(parse_duration("30"), Ok(Duration::from_secs(30)));

        // Whitespace
        assert_eq!(parse_duration("  30s  "), Ok(Duration::from_secs(30)));

        // Error cases
        assert!(parse_duration("abc").is_err());
        assert!(parse_duration("10x").is_err());
        assert!(parse_duration("").is_err());
        assert!(parse_duration("ms").is_err());
    }
}
