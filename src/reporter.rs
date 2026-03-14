use crate::cli::Args;
use crate::stats::AggregatedStats;
use colored::*;
use std::time::Duration;

pub fn print_header(args: &Args) {
    println!();
    println!(
        "{}",
        "  ⚡ rench — HTTP benchmark tool".bright_cyan().bold()
    );
    println!("{}", "  ─────────────────────────────────".bright_black());
    println!(
        "  {} {}",
        "Target:".bright_white().bold(),
        args.url.bright_yellow()
    );
    println!(
        "  {} {} connections, {} duration",
        "Config:".bright_white().bold(),
        args.connections.to_string().bright_green(),
        format_duration(args.duration).bright_green(),
    );
    if args.rate > 0 {
        println!(
            "  {} {} req/s",
            "Rate limit:".bright_white().bold(),
            args.rate.to_string().bright_green(),
        );
    }
    println!(
        "  {} {} {}",
        "Request:".bright_white().bold(),
        args.method.bright_magenta(),
        if args.http2 { "HTTP/2" } else { "HTTP/1.1" }.bright_blue()
    );
    println!("{}", "  ─────────────────────────────────".bright_black());
    if args.insecure {
        println!(
            "  {} {}",
            "WARNING:".bright_yellow().bold(),
            "TLS certificate verification is disabled!".bright_yellow()
        );
    }
    println!();
}

pub fn print_progress(
    elapsed: Duration,
    total_duration: Duration,
    requests: u64,
    errors: u64,
    bytes: u64,
    current_rps: f64,
) {
    let pct = (elapsed.as_secs_f64() / total_duration.as_secs_f64() * 100.0).min(100.0);

    // Build a progress bar
    let bar_width = 20;
    let filled = ((pct / 100.0) * bar_width as f64) as usize;
    let empty = bar_width - filled;
    let bar = format!("{}{}", "█".repeat(filled), "░".repeat(empty));

    let err_str = if errors > 0 {
        format!("  {} err", errors.to_string().red())
    } else {
        String::new()
    };

    eprint!(
        "\r  {} {} {:.0}%  {} req  {:.0} req/s  {}{}",
        format_duration(elapsed).bright_white(),
        bar.bright_cyan(),
        pct,
        format_number(requests).bright_green(),
        current_rps,
        format_bytes(bytes).bright_blue(),
        err_str,
    );
}

pub fn print_results(stats: &AggregatedStats, show_latency_dist: bool) {
    println!();
    println!();
    println!(
        "{}",
        "  ──────────── Results ────────────".bright_cyan().bold()
    );
    println!();

    // Throughput
    println!("  {}", "Throughput".bright_white().bold().underline());
    println!(
        "    {} {:>12.2} req/s",
        "Requests/sec:".bright_white(),
        stats.rps()
    );
    println!(
        "    {} {:>12.2} Mbps",
        "Transfer:    ".bright_white(),
        stats.throughput_mbps()
    );
    println!(
        "    {} {:>12}",
        "Total reqs:  ".bright_white(),
        format_number(stats.total_requests)
    );
    println!(
        "    {} {:>12}",
        "Total data:  ".bright_white(),
        format_bytes(stats.bytes_received)
    );
    println!(
        "    {} {:>12}",
        "Duration:    ".bright_white(),
        format_duration(stats.elapsed)
    );
    println!();

    // Latency
    if stats.latency_hist.len() > 0 {
        println!("  {}", "Latency".bright_white().bold().underline());
        println!(
            "    {} {:>12}",
            "Min:   ".bright_white(),
            format_micros(stats.latency_hist.min())
        );
        println!(
            "    {} {:>12}",
            "Mean:  ".bright_white(),
            format_micros(stats.latency_hist.mean() as u64)
        );
        println!(
            "    {} {:>12}",
            "p50:   ".bright_white(),
            format_micros(stats.latency_hist.value_at_quantile(0.50))
        );
        println!(
            "    {} {:>12}",
            "p90:   ".bright_white(),
            format_micros(stats.latency_hist.value_at_quantile(0.90))
        );
        println!(
            "    {} {:>12}",
            "p99:   ".bright_white(),
            format_micros(stats.latency_hist.value_at_quantile(0.99))
        );
        println!(
            "    {} {:>12}",
            "Max:   ".bright_white(),
            format_micros(stats.latency_hist.max())
        );
        println!(
            "    {} {:>12}",
            "StdDev:".bright_white(),
            format_micros(stats.latency_hist.stdev() as u64)
        );
        println!();

        if show_latency_dist {
            print_latency_distribution(stats);
        }
    }

    // Status codes
    println!("  {}", "Status Codes".bright_white().bold().underline());
    if stats.status_2xx > 0 {
        println!(
            "    {} {}",
            "[2xx]".bright_green().bold(),
            format_number(stats.status_2xx)
        );
    }
    if stats.status_3xx > 0 {
        println!(
            "    {} {}",
            "[3xx]".bright_yellow().bold(),
            format_number(stats.status_3xx)
        );
    }
    if stats.status_4xx > 0 {
        println!(
            "    {} {}",
            "[4xx]".bright_red().bold(),
            format_number(stats.status_4xx)
        );
    }
    if stats.status_5xx > 0 {
        println!(
            "    {} {}",
            "[5xx]".red().bold(),
            format_number(stats.status_5xx)
        );
    }
    println!();

    // Errors
    let total_errors = stats.total_errors();
    if total_errors > 0 {
        println!("  {}", "Errors".bright_red().bold().underline());
        if stats.connect_errors > 0 {
            println!(
                "    {} {}",
                "Connect:".bright_white(),
                stats.connect_errors.to_string().red()
            );
        }
        if stats.read_errors > 0 {
            println!(
                "    {} {}",
                "Read:   ".bright_white(),
                stats.read_errors.to_string().red()
            );
        }
        if stats.timeout_errors > 0 {
            println!(
                "    {} {}",
                "Timeout:".bright_white(),
                stats.timeout_errors.to_string().red()
            );
        }
        println!();
    }

    // Success rate
    let rate = stats.success_rate();
    let rate_color = if rate >= 99.0 {
        rate.to_string().bright_green()
    } else if rate >= 95.0 {
        rate.to_string().bright_yellow()
    } else {
        rate.to_string().bright_red()
    };
    println!(
        "  {} {:.1}%",
        "Success Rate:".bright_white().bold(),
        rate_color
    );
    println!();
}

fn print_latency_distribution(stats: &AggregatedStats) {
    println!(
        "  {}",
        "Latency Distribution".bright_white().bold().underline()
    );

    let percentiles = [
        10.0, 25.0, 50.0, 75.0, 90.0, 95.0, 99.0, 99.5, 99.9, 99.99,
    ];
    let max_val = stats.latency_hist.value_at_quantile(0.9999) as f64;

    for p in &percentiles {
        let val = stats.latency_hist.value_at_quantile(*p / 100.0);
        let bar_len = if max_val > 0.0 {
            ((val as f64 / max_val) * 30.0) as usize
        } else {
            0
        };
        let bar = "█".repeat(bar_len);
        println!(
            "    {:>6.2}% {:>10} {}",
            p,
            format_micros(val),
            bar.bright_cyan()
        );
    }
    println!();
}

fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    if secs >= 60 {
        format!("{}m{}s", secs / 60, secs % 60)
    } else if secs > 0 {
        format!("{:.1}s", d.as_secs_f64())
    } else {
        format!("{}ms", d.as_millis())
    }
}

fn format_micros(us: u64) -> String {
    if us >= 1_000_000 {
        format!("{:.2}s", us as f64 / 1_000_000.0)
    } else if us >= 1_000 {
        format!("{:.2}ms", us as f64 / 1_000.0)
    } else {
        format!("{}µs", us)
    }
}

fn format_number(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.2}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.2}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn format_bytes(b: u64) -> String {
    if b >= 1_073_741_824 {
        format!("{:.2} GB", b as f64 / 1_073_741_824.0)
    } else if b >= 1_048_576 {
        format!("{:.2} MB", b as f64 / 1_048_576.0)
    } else if b >= 1_024 {
        format!("{:.2} KB", b as f64 / 1_024.0)
    } else {
        format!("{} B", b)
    }
}
