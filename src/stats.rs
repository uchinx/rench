use hdrhistogram::Histogram;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Per-worker stats collected lock-free during the benchmark run.
#[derive(Debug)]
pub struct WorkerStats {
    pub latency_hist: Histogram<u64>,
    pub status_2xx: u64,
    pub status_3xx: u64,
    pub status_4xx: u64,
    pub status_5xx: u64,
    pub connect_errors: u64,
    pub read_errors: u64,
    pub timeout_errors: u64,
    pub bytes_received: u64,
    pub requests_sent: u64,
}

impl WorkerStats {
    pub fn new() -> Self {
        Self {
            // Track latencies from 1µs to 60s with 3 significant digits
            latency_hist: Histogram::new_with_bounds(1, 60_000_000, 3).unwrap(),
            status_2xx: 0,
            status_3xx: 0,
            status_4xx: 0,
            status_5xx: 0,
            connect_errors: 0,
            read_errors: 0,
            timeout_errors: 0,
            bytes_received: 0,
            requests_sent: 0,
        }
    }

    pub fn record_latency(&mut self, latency: Duration) {
        let micros = latency.as_micros() as u64;
        let micros = micros.clamp(1, 60_000_000);
        let _ = self.latency_hist.record(micros);
    }

    pub fn record_status(&mut self, status: u16) {
        match status / 100 {
            2 => self.status_2xx += 1,
            3 => self.status_3xx += 1,
            4 => self.status_4xx += 1,
            5 => self.status_5xx += 1,
            _ => {}
        }
        self.requests_sent += 1;
    }

    pub fn record_error(&mut self, kind: ErrorKind) {
        match kind {
            ErrorKind::Connect => self.connect_errors += 1,
            ErrorKind::Read => self.read_errors += 1,
            ErrorKind::Timeout => self.timeout_errors += 1,
        }
        self.requests_sent += 1;
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ErrorKind {
    Connect,
    Read,
    Timeout,
}

/// Aggregated results from all workers.
pub struct AggregatedStats {
    pub latency_hist: Histogram<u64>,
    pub total_requests: u64,
    pub status_2xx: u64,
    pub status_3xx: u64,
    pub status_4xx: u64,
    pub status_5xx: u64,
    pub connect_errors: u64,
    pub read_errors: u64,
    pub timeout_errors: u64,
    pub bytes_received: u64,
    pub elapsed: Duration,
}

impl AggregatedStats {
    pub fn merge(workers: Vec<WorkerStats>, elapsed: Duration) -> Self {
        let mut agg = AggregatedStats {
            latency_hist: Histogram::new_with_bounds(1, 60_000_000, 3).unwrap(),
            total_requests: 0,
            status_2xx: 0,
            status_3xx: 0,
            status_4xx: 0,
            status_5xx: 0,
            connect_errors: 0,
            read_errors: 0,
            timeout_errors: 0,
            bytes_received: 0,
            elapsed,
        };

        for w in workers {
            agg.latency_hist.add(&w.latency_hist).ok();
            agg.total_requests += w.requests_sent;
            agg.status_2xx += w.status_2xx;
            agg.status_3xx += w.status_3xx;
            agg.status_4xx += w.status_4xx;
            agg.status_5xx += w.status_5xx;
            agg.connect_errors += w.connect_errors;
            agg.read_errors += w.read_errors;
            agg.timeout_errors += w.timeout_errors;
            agg.bytes_received += w.bytes_received;
        }

        agg
    }

    pub fn rps(&self) -> f64 {
        self.total_requests as f64 / self.elapsed.as_secs_f64()
    }

    pub fn throughput_mbps(&self) -> f64 {
        (self.bytes_received as f64 * 8.0) / (self.elapsed.as_secs_f64() * 1_000_000.0)
    }

    pub fn total_errors(&self) -> u64 {
        self.connect_errors + self.read_errors + self.timeout_errors
    }

    pub fn success_rate(&self) -> f64 {
        if self.total_requests == 0 {
            return 0.0;
        }
        self.status_2xx as f64 / self.total_requests as f64 * 100.0
    }
}

/// Shared live counters updated by workers, read by the progress reporter.
pub struct LiveStats {
    pub requests: AtomicU64,
    pub errors: AtomicU64,
    pub bytes: AtomicU64,
}

impl LiveStats {
    pub fn new() -> Self {
        Self {
            requests: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
        }
    }
}

/// Global atomic counter for coordinating total request limits across workers.
pub struct RequestCounter {
    remaining: AtomicU64,
    unlimited: bool,
}

impl RequestCounter {
    pub fn new(total: u64) -> Self {
        Self {
            remaining: AtomicU64::new(total),
            unlimited: total == 0,
        }
    }

    /// Try to claim one request slot. Returns false if limit reached.
    pub fn try_acquire(&self) -> bool {
        if self.unlimited {
            return true;
        }
        loop {
            let current = self.remaining.load(Ordering::Relaxed);
            if current == 0 {
                return false;
            }
            if self
                .remaining
                .compare_exchange_weak(current, current - 1, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                return true;
            }
        }
    }
}
