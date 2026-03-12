# ⚡ rench

A high-performance HTTP benchmarking tool built in Rust.

Uses **tokio** + **hyper** for fully async I/O with connection pooling, and **hdrhistogram** for accurate latency percentile tracking across millions of requests.

## Features

- **Async I/O** — tokio multi-threaded runtime with hyper's connection pool
- **HDR Histogram** — accurate latency percentiles (p50/p90/p99/p99.9) with 3 significant digits
- **HTTP/1.1 & HTTP/2** — protocol selection via `--http2`
- **TLS support** — rustls-based, with `--insecure` to skip cert verification
- **Rate limiting** — constrain throughput with `--rate` (requests/sec)
- **Lock-free counters** — atomic request coordination across workers, zero contention
- **Custom requests** — headers (`-H`), body (`-b`), body from file (`-B`), any HTTP method
- **Colored output** — clean terminal reports with latency distribution bars

## Installation

```bash
cargo install --path .
```

## Usage

```bash
# Basic: 50 connections for 10 seconds (defaults)
rench http://localhost:8080

# 200 connections for 30 seconds with latency distribution
rench -c 200 -d 30s --latency http://localhost:8080/api

# Fixed number of requests
rench -c 100 -n 10000 http://localhost:8080

# Rate-limited at 5000 req/s
rench -c 50 --rate 5000 http://localhost:8080

# POST with body and custom headers
rench -m POST -b '{"key":"value"}' \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer token123" \
  https://api.example.com/endpoint

# POST with body from file
rench -m POST -B payload.json \
  -H "Content-Type: application/json" \
  https://api.example.com/endpoint

# HTTP/2 with TLS cert skip
rench --http2 -k https://localhost:8443

# Control thread count
rench -t 8 -c 500 -d 60s http://localhost:8080
```

## CLI Reference

| Flag               | Default | Description                              |
|---------------------|---------|------------------------------------------|
| `-c, --connections` | 50      | Number of concurrent connections         |
| `-n, --requests`    | 0       | Total requests (0 = unlimited)           |
| `-d, --duration`    | 10s     | Benchmark duration (e.g. 10s, 1m)        |
| `-t, --threads`     | CPUs    | Worker threads                           |
| `-m, --method`      | GET     | HTTP method                              |
| `-H, --header`      |         | Custom header (repeatable)               |
| `-b, --body`        |         | Request body string                      |
| `-B, --body-file`   |         | Read body from file                      |
| `--http2`           | false   | Force HTTP/2                             |
| `--rate`            | 0       | Target requests/sec (0 = unlimited)      |
| `--timeout`         | 5s      | Connection timeout                       |
| `-k, --insecure`    | false   | Skip TLS certificate verification        |
| `--latency`         | false   | Show full latency distribution           |

## Architecture

```
main
 └─ tokio runtime (N worker threads)
     ├─ worker 0 ──→ hyper Client (connection pool) ──→ target
     ├─ worker 1 ──→ hyper Client (connection pool) ──→ target
     ├─ ...
     └─ worker C ──→ hyper Client (connection pool) ──→ target

Each worker:
  1. Acquires slot from atomic RequestCounter
  2. Builds + sends request via pooled hyper client
  3. Records latency in local HDR Histogram
  4. Tracks status codes + errors locally
  5. Repeats until duration/count exhausted

Post-run: merge all histograms + counters → report
```

## Performance Notes

- Each worker owns its own `hyper::Client` with connection pooling — no mutex contention
- Per-worker `WorkerStats` with thread-local HDR histograms — merge only at the end
- Atomic `RequestCounter` uses `compare_exchange_weak` for minimal CAS overhead
- Release profile uses LTO + single codegen unit + native CPU targeting
- TCP_NODELAY enabled on all connections

## License

MIT
