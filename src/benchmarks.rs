/// Performance benchmarks for continuous query system
/// 
/// Target: <16ms per query for 60 FPS gameplay
/// Tests cache performance, generation overhead, and memory usage

use crate::continuous_world::ContinuousWorld;
use crate::spatial_index::AABB;
use std::time::Instant;

// Test location: Kangaroo Point, Brisbane (-27.479769°, 153.033586°)
const KANGAROO_POINT: [f64; 3] = [-5046877.97, 2567787.42, -2925481.59];

/// Benchmark single query performance (cache cold)
pub fn bench_cold_query() -> Result<f64, Box<dyn std::error::Error>> {
    let mut world = ContinuousWorld::new(KANGAROO_POINT, 100.0)?;
    
    let query = AABB::from_center(KANGAROO_POINT, 10.0);
    
    let start = Instant::now();
    let blocks = world.query_range(query);
    let elapsed = start.elapsed();
    
    println!("Cold query: {} blocks in {:?} ({:.2}ms)", 
        blocks.len(), elapsed, elapsed.as_secs_f64() * 1000.0);
    
    Ok(elapsed.as_secs_f64() * 1000.0)
}

/// Benchmark cache hit performance
pub fn bench_cache_hit() -> Result<f64, Box<dyn std::error::Error>> {
    let mut world = ContinuousWorld::new(KANGAROO_POINT, 100.0)?;
    
    let query = AABB::from_center(KANGAROO_POINT, 10.0);
    
    // Prime the cache
    let _ = world.query_range(query);
    
    // Benchmark cache hit
    let start = Instant::now();
    let blocks = world.query_range(query);
    let elapsed = start.elapsed();
    
    println!("Cache hit: {} blocks in {:?} ({:.2}ms)", 
        blocks.len(), elapsed, elapsed.as_secs_f64() * 1000.0);
    
    Ok(elapsed.as_secs_f64() * 1000.0)
}

/// Benchmark moving query (simulates player movement)
pub fn bench_moving_query() -> Result<Vec<f64>, Box<dyn std::error::Error>> {
    let mut world = ContinuousWorld::new(KANGAROO_POINT, 100.0)?;
    
    let mut times = Vec::new();
    let step = 5.0; // 5m steps
    
    println!("\nMoving query benchmark (10m radius, 5m steps):");
    
    for i in 0..10 {
        let offset = [step * i as f64, 0.0, 0.0];
        let center = [
            KANGAROO_POINT[0] + offset[0],
            KANGAROO_POINT[1] + offset[1],
            KANGAROO_POINT[2] + offset[2],
        ];
        let query = AABB::from_center(center, 10.0);
        
        let start = Instant::now();
        let blocks = world.query_range(query);
        let elapsed = start.elapsed();
        let ms = elapsed.as_secs_f64() * 1000.0;
        
        times.push(ms);
        println!("  Step {}: {} blocks in {:.2}ms", i, blocks.len(), ms);
    }
    
    Ok(times)
}

/// Benchmark different query sizes
pub fn bench_query_sizes() -> Result<Vec<(f64, f64)>, Box<dyn std::error::Error>> {
    let mut world = ContinuousWorld::new(KANGAROO_POINT, 100.0)?;
    
    let radii = vec![5.0, 10.0, 20.0, 50.0];
    let mut results = Vec::new();
    
    println!("\nQuery size benchmark:");
    
    for radius in radii {
        let query = AABB::from_center(KANGAROO_POINT, radius);
        
        let start = Instant::now();
        let blocks = world.query_range(query);
        let elapsed = start.elapsed();
        let ms = elapsed.as_secs_f64() * 1000.0;
        
        results.push((radius, ms));
        println!("  {}m radius: {} blocks in {:.2}ms", radius, blocks.len(), ms);
    }
    
    Ok(results)
}

/// Estimate memory usage
pub fn bench_memory_usage() -> Result<usize, Box<dyn std::error::Error>> {
    let mut world = ContinuousWorld::new(KANGAROO_POINT, 100.0)?;
    
    // Fill cache with queries
    let radius = 50.0;
    let query = AABB::from_center(KANGAROO_POINT, radius);
    let blocks = world.query_range(query);
    
    // Estimate: each block = 512³ voxels = ~1KB compressed
    let block_size_kb = 1;
    let total_kb = blocks.len() * block_size_kb;
    
    println!("\nMemory usage estimate:");
    println!("  Blocks in cache: {}", blocks.len());
    println!("  Estimated memory: ~{}KB ({:.2}MB)", total_kb, total_kb as f64 / 1024.0);
    
    Ok(total_kb)
}

/// Run all benchmarks
pub fn run_all_benchmarks() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Continuous Query System Benchmarks ===");
    println!("Target: <16ms per query (60 FPS)");
    println!("Location: Kangaroo Point, Brisbane\n");
    
    // Cold query
    let cold_ms = bench_cold_query()?;
    
    // Cache hit
    let hit_ms = bench_cache_hit()?;
    
    // Moving query
    let move_times = bench_moving_query()?;
    let avg_move = move_times.iter().sum::<f64>() / move_times.len() as f64;
    
    // Query sizes
    let size_results = bench_query_sizes()?;
    
    // Memory
    let memory_kb = bench_memory_usage()?;
    
    // Summary
    println!("\n=== Summary ===");
    println!("Cold query: {:.2}ms {}", cold_ms, 
        if cold_ms < 16.0 { "✅ PASS" } else { "❌ FAIL" });
    println!("Cache hit: {:.2}ms {}", hit_ms, 
        if hit_ms < 16.0 { "✅ PASS" } else { "❌ FAIL" });
    println!("Moving average: {:.2}ms {}", avg_move, 
        if avg_move < 16.0 { "✅ PASS" } else { "❌ FAIL" });
    println!("Memory usage: ~{}KB ({:.2}MB)", memory_kb, memory_kb as f64 / 1024.0);
    
    // Performance analysis
    println!("\n=== Analysis ===");
    if hit_ms < 1.0 {
        println!("✅ Cache performance: EXCELLENT (<1ms)");
    } else if hit_ms < 5.0 {
        println!("✅ Cache performance: GOOD (<5ms)");
    } else {
        println!("⚠️  Cache performance: NEEDS OPTIMIZATION (>5ms)");
    }
    
    if cold_ms < 50.0 {
        println!("✅ Generation overhead: ACCEPTABLE (<50ms)");
    } else if cold_ms < 100.0 {
        println!("⚠️  Generation overhead: HIGH (50-100ms)");
    } else {
        println!("❌ Generation overhead: CRITICAL (>100ms)");
    }
    
    println!("\nTarget met: {}", 
        if hit_ms < 16.0 && avg_move < 16.0 { 
            "✅ YES - Ready for gameplay!" 
        } else { 
            "❌ NO - Optimization needed" 
        });
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_cold_query_benchmark() {
        let result = bench_cold_query();
        assert!(result.is_ok(), "Cold query benchmark should succeed");
    }
    
    #[test]
    fn test_cache_hit_benchmark() {
        let result = bench_cache_hit();
        assert!(result.is_ok(), "Cache hit benchmark should succeed");
        
        let ms = result.unwrap();
        assert!(ms < 100.0, "Cache hit should be <100ms (got {:.2}ms)", ms);
    }
    
    #[test]
    fn test_moving_query_benchmark() {
        let result = bench_moving_query();
        assert!(result.is_ok(), "Moving query benchmark should succeed");
    }
    
    #[test]
    fn test_query_sizes_benchmark() {
        let result = bench_query_sizes();
        assert!(result.is_ok(), "Query sizes benchmark should succeed");
    }
    
    #[test]
    fn test_memory_usage_estimate() {
        let result = bench_memory_usage();
        assert!(result.is_ok(), "Memory usage estimate should succeed");
        
        let kb = result.unwrap();
        assert!(kb < 10_000, "Memory usage should be reasonable (<10MB)");
    }
    
    #[test]
    fn test_performance_target() {
        // Validate we meet the <16ms target for cache hits
        let result = bench_cache_hit();
        assert!(result.is_ok(), "Benchmark should succeed");
        
        let ms = result.unwrap();
        println!("Cache hit performance: {:.2}ms", ms);
        
        // This is the critical gameplay performance metric
        assert!(ms < 16.0, 
            "Cache hit MUST be <16ms for 60 FPS (got {:.2}ms)", ms);
    }
}
