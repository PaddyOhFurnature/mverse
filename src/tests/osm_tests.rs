use crate::osm::{OverpassClient, parse_overpass_response, assign_osm_to_chunks, RoadType, OsmData, OsmBuilding, OsmRoad};
use crate::coordinates::GpsPos;
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

// Phase 4.4 tests - Chunk assignment

#[test]
fn test_building_fully_in_one_chunk() {
    // Create a small building in a single chunk
    let mut data = OsmData::default();
    data.buildings.push(OsmBuilding {
        id: 1,
        polygon: vec![
            GpsPos { lat_deg: -27.470, lon_deg: 153.025, elevation_m: 0.0 },
            GpsPos { lat_deg: -27.470, lon_deg: 153.026, elevation_m: 0.0 },
            GpsPos { lat_deg: -27.471, lon_deg: 153.026, elevation_m: 0.0 },
            GpsPos { lat_deg: -27.471, lon_deg: 153.025, elevation_m: 0.0 },
        ],
        height_m: 10.0,
        building_type: "yes".to_string(),
        levels: 3,
    });
    
    let chunks = assign_osm_to_chunks(&data, 10);
    
    // Building should be assigned to exactly one chunk (by centroid)
    assert_eq!(chunks.len(), 1);
    let chunk_data = chunks.values().next().unwrap();
    assert_eq!(chunk_data.buildings.len(), 1);
    assert_eq!(chunk_data.buildings[0].id, 1);
}

#[test]
fn test_road_crossing_chunk_boundary() {
    // Create a road that spans multiple chunks
    let mut data = OsmData::default();
    data.roads.push(OsmRoad {
        id: 1,
        nodes: vec![
            GpsPos { lat_deg: -27.0, lon_deg: 153.0, elevation_m: 0.0 },
            GpsPos { lat_deg: -27.5, lon_deg: 153.5, elevation_m: 0.0 }, // Far apart
        ],
        road_type: RoadType::Residential,
        width_m: 6.0,
        name: None,
    });
    
    let chunks = assign_osm_to_chunks(&data, 10);
    
    // Road should be in at least 2 chunks (likely many more given distance)
    assert!(chunks.len() >= 2, "Expected road in multiple chunks, got {}", chunks.len());
    
    // Each chunk should have the road
    for chunk_data in chunks.values() {
        assert_eq!(chunk_data.roads.len(), 1);
        assert_eq!(chunk_data.roads[0].id, 1);
    }
}

#[test]
fn test_all_entities_assigned_to_at_least_one_chunk() {
    let json: serde_json::Value = serde_json::from_str(FIXTURE_JSON).unwrap();
    let data = parse_overpass_response(&json).unwrap();
    
    let chunks = assign_osm_to_chunks(&data, 10);
    
    // Count total entities across all chunks
    let mut total_buildings = 0;
    let mut total_roads = 0;
    let mut total_water = 0;
    let mut total_parks = 0;
    
    for chunk_data in chunks.values() {
        total_buildings += chunk_data.buildings.len();
        total_roads += chunk_data.roads.len();
        total_water += chunk_data.water.len();
        total_parks += chunk_data.parks.len();
    }
    
    // All entities should be assigned (buildings/water/parks: 1 chunk each, roads: possibly multiple)
    assert!(total_buildings >= data.buildings.len());
    assert!(total_roads >= data.roads.len());
    assert!(total_water >= data.water.len());
    assert!(total_parks >= data.parks.len());
}

#[test]
fn test_no_entities_lost_in_assignment() {
    let json: serde_json::Value = serde_json::from_str(FIXTURE_JSON).unwrap();
    let data = parse_overpass_response(&json).unwrap();
    
    let original_building_count = data.buildings.len();
    let original_road_count = data.roads.len();
    
    let chunks = assign_osm_to_chunks(&data, 10);
    
    // Collect unique building IDs across all chunks
    let mut building_ids = std::collections::HashSet::new();
    let mut road_ids = std::collections::HashSet::new();
    
    for chunk_data in chunks.values() {
        for building in &chunk_data.buildings {
            building_ids.insert(building.id);
        }
        for road in &chunk_data.roads {
            road_ids.insert(road.id);
        }
    }
    
    // No entities should be lost
    assert_eq!(building_ids.len(), original_building_count);
    assert_eq!(road_ids.len(), original_road_count);
}
