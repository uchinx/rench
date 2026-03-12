mod cli;
mod engine;
mod reporter;
mod stats;

use clap::Parser;
use cli::Args;
use stats::AggregatedStats;
use std::process;

fn main() {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    let args = Args::parse();

    // Validate URL
    if args.url.parse::<http::Uri>().is_err() {
        eprintln!("Error: invalid URL '{}'", args.url);
        process::exit(1);
    }

    // Determine thread count
    let threads = if args.threads > 0 {
        args.threads
    } else {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
    };

    // Build runtime with configured thread pool
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(threads)
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime");

    reporter::print_header(&args);

    let result = runtime.block_on(async {
        engine::run(&args).await
    });

    match result {
        Ok((worker_stats, elapsed)) => {
            let aggregated = AggregatedStats::merge(worker_stats, elapsed);
            reporter::print_results(&aggregated, args.latency);
        }
        Err(e) => {
            eprintln!("\n  {} {}", "Error:".bright_red(), e);
            process::exit(1);
        }
    }
}

use colored::Colorize;
