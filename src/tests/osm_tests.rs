use crate::osm::{OverpassClient, parse_overpass_response, RoadType};
use std::time::{Duration, Instant};

const FIXTURE_JSON: &str = include_str!("../../tests/fixtures/brisbane_cbd.json");

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

// Phase 4.3 tests - OSM data parser

#[test]
fn test_parse_fixture_has_buildings() {
    let json: serde_json::Value = serde_json::from_str(FIXTURE_JSON).unwrap();
    let data = parse_overpass_response(&json).unwrap();
    
    assert_eq!(data.buildings.len(), 2, "Expected 2 buildings in fixture");
    
    // First building has explicit height
    let building1 = &data.buildings[0];
    assert_eq!(building1.id, 100);
    assert_eq!(building1.building_type, "commercial");
    assert_eq!(building1.levels, 5);
    assert_eq!(building1.height_m, 18.0);
    assert_eq!(building1.polygon.len(), 5); // 4 corners + closing
}

#[test]
fn test_parse_fixture_has_roads() {
    let json: serde_json::Value = serde_json::from_str(FIXTURE_JSON).unwrap();
    let data = parse_overpass_response(&json).unwrap();
    
    assert_eq!(data.roads.len(), 2, "Expected 2 roads in fixture");
    
    // First road is motorway with explicit width
    let road1 = &data.roads[0];
    assert_eq!(road1.id, 200);
    assert_eq!(road1.road_type, RoadType::Motorway);
    assert_eq!(road1.width_m, 15.0);
    assert_eq!(road1.name, Some("Pacific Motorway".to_string()));
}

#[test]
fn test_missing_height_uses_default() {
    let json: serde_json::Value = serde_json::from_str(FIXTURE_JSON).unwrap();
    let data = parse_overpass_response(&json).unwrap();
    
    // Second building has no height/levels tags
    let building2 = &data.buildings[1];
    assert_eq!(building2.id, 101);
    assert_eq!(building2.levels, 3); // Default
    assert_eq!(building2.height_m, 9.0); // 3 levels * 3m
}

#[test]
fn test_road_classification() {
    assert_eq!(RoadType::from_highway_tag("motorway"), RoadType::Motorway);
    assert_eq!(RoadType::from_highway_tag("motorway_link"), RoadType::Motorway);
    assert_eq!(RoadType::from_highway_tag("residential"), RoadType::Residential);
    assert_eq!(RoadType::from_highway_tag("footway"), RoadType::Path);
    
    match RoadType::from_highway_tag("unknown") {
        RoadType::Other(s) => assert_eq!(s, "unknown"),
        _ => panic!("Expected Other variant"),
    }
}

#[test]
fn test_road_default_widths() {
    assert_eq!(RoadType::Motorway.default_width_m(), 12.0);
    assert_eq!(RoadType::Residential.default_width_m(), 6.0);
    assert_eq!(RoadType::Path.default_width_m(), 2.0);
}

#[test]
fn test_parse_malformed_json_returns_error() {
    let json: serde_json::Value = serde_json::json!({"invalid": "structure"});
    let result = parse_overpass_response(&json);
    assert!(result.is_err());
}

#[test]
fn test_parse_empty_response() {
    let json: serde_json::Value = serde_json::json!({"elements": []});
    let result = parse_overpass_response(&json);
    assert!(result.is_ok());
    
    let data = result.unwrap();
    assert_eq!(data.buildings.len(), 0);
    assert_eq!(data.roads.len(), 0);
    assert_eq!(data.water.len(), 0);
    assert_eq!(data.parks.len(), 0);
}
