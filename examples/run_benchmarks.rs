/// Standalone benchmark runner for continuous query system
/// 
/// Run with: cargo run --example run_benchmarks --release

use metaverse_core::benchmarks::run_all_benchmarks;

fn main() {
    match run_all_benchmarks() {
        Ok(_) => {
            println!("\nBenchmarks completed successfully!");
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("\nBenchmark failed: {}", e);
            std::process::exit(1);
        }
    }
}
