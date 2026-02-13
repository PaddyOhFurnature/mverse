use crate::osm::OverpassClient;
use std::time::{Duration, Instant};

#[test]
fn test_query_builder_produces_valid_overpass_ql() {
    let query = OverpassClient::build_query(-27.475, 153.020, -27.465, 153.035);
    
    // Should contain all required elements
    assert!(query.contains("[out:json][timeout:25]"));
    assert!(query.contains("way[\"building\"]"));
    assert!(query.contains("way[\"highway\"]"));
    assert!(query.contains("way[\"natural\"=\"water\"]"));
    assert!(query.contains("way[\"leisure\"=\"park\"]"));
    assert!(query.contains("relation[\"natural\"=\"water\"]"));
    assert!(query.contains("out body"));
    assert!(query.contains("out skel qt"));
    
    // Should contain the bbox coordinates (trailing zeros may be dropped)
    assert!(query.contains("-27.475"));
    assert!(query.contains("153.02")); // 153.020 loses trailing zero
    assert!(query.contains("-27.465"));
    assert!(query.contains("153.035"));
}

#[test]
fn test_rate_limiter_enforces_cooldown() {
    // Test by checking timestamps directly
    let client = OverpassClient::with_endpoint(2, "http://localhost:9999".to_string());
    
    let start = Instant::now();
    
    // Manually trigger wait_for_cooldown
    client.wait_for_cooldown();
    let first_elapsed = start.elapsed();
    
    // Second call should wait at least 2 seconds from start
    client.wait_for_cooldown();
    let second_elapsed = start.elapsed();
    
    // First call should be immediate (no prior request)
    assert!(first_elapsed < Duration::from_millis(100));
    
    // Second call should have waited for cooldown
    assert!(second_elapsed >= Duration::from_secs(2), 
        "Expected wait ≥ 2s, got {:?}", second_elapsed);
}

#[test]
fn test_backoff_sequence_doubles_correctly() {
    // Backoff should be: 3s, 6s, 12s, 24s, 48s, 60s (capped)
    assert_eq!(OverpassClient::backoff_duration(0), Duration::from_secs(3));
    assert_eq!(OverpassClient::backoff_duration(1), Duration::from_secs(6));
    assert_eq!(OverpassClient::backoff_duration(2), Duration::from_secs(12));
    assert_eq!(OverpassClient::backoff_duration(3), Duration::from_secs(24));
    assert_eq!(OverpassClient::backoff_duration(4), Duration::from_secs(48));
    assert_eq!(OverpassClient::backoff_duration(5), Duration::from_secs(60)); // 96 capped to 60
    assert_eq!(OverpassClient::backoff_duration(10), Duration::from_secs(60)); // Always capped
}

#[test]
#[ignore] // Integration test - requires network
fn test_fetch_brisbane_cbd_integration() {
    let client = OverpassClient::new(3);
    
    // Small bbox in Brisbane CBD
    let result = client.query_bbox(-27.475, 153.020, -27.465, 153.035);
    
    assert!(result.is_ok(), "Query failed: {:?}", result.err());
    
    let json = result.unwrap();
    assert!(json.is_object());
    
    // Should have elements array
    let elements = json.get("elements");
    assert!(elements.is_some());
    assert!(elements.unwrap().is_array());
    
    // Should have at least some data in Brisbane CBD
    let count = elements.unwrap().as_array().unwrap().len();
    assert!(count > 0, "Expected some OSM elements in Brisbane CBD, got {}", count);
}
