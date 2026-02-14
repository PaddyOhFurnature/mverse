//! Automated screenshot tester
//! 
//! Takes screenshots from 10 different camera positions/angles
//! Saves them with descriptive names so we can see what's actually rendering

use metaverse_core::renderer::{camera::Camera, pipeline::BasicPipeline, Renderer};
use metaverse_core::osm::OsmData;
use metaverse_core::cache::DiskCache;
use metaverse_core::svo_integration::generate_mesh_from_osm;
use metaverse_core::coordinates::{gps_to_ecef, GpsPos};
use std::sync::Arc;
use wgpu::util::DeviceExt;
use glam::DVec3;

struct TestView {
    name: &'static str,
    position: GpsPos,
    look_at: GpsPos,
    description: &'static str,
}

const TEST_VIEWS: &[TestView] = &[
    TestView {
        name: "01_spawn_ground_level",
        position: GpsPos { lat_deg: -27.4698, lon_deg: 153.0251, elevation_m: 2.0 },
        look_at: GpsPos { lat_deg: -27.4708, lon_deg: 153.0251, elevation_m: 2.0 },
        description: "Ground level looking north at buildings",
    },
    TestView {
        name: "02_eye_level_50m",
        position: GpsPos { lat_deg: -27.4698, lon_deg: 153.0251, elevation_m: 50.0 },
        look_at: GpsPos { lat_deg: -27.4708, lon_deg: 153.0261, elevation_m: 20.0 },
        description: "Eye level 50m looking at CBD",
    },
    TestView {
        name: "03_low_angle_100m",
        position: GpsPos { lat_deg: -27.4698, lon_deg: 153.0251, elevation_m: 100.0 },
        look_at: GpsPos { lat_deg: -27.4718, lon_deg: 153.0271, elevation_m: 30.0 },
        description: "100m altitude, angled view",
    },
    TestView {
        name: "04_street_view_5m",
        position: GpsPos { lat_deg: -27.4698, lon_deg: 153.0251, elevation_m: 5.0 },
        look_at: GpsPos { lat_deg: -27.4698, lon_deg: 153.0261, elevation_m: 15.0 },
        description: "Street level looking east",
    },
    TestView {
        name: "05_rooftop_30m",
        position: GpsPos { lat_deg: -27.4708, lon_deg: 153.0241, elevation_m: 30.0 },
        look_at: GpsPos { lat_deg: -27.4688, lon_deg: 153.0251, elevation_m: 10.0 },
        description: "Rooftop view looking south",
    },
    TestView {
        name: "06_aerial_500m",
        position: GpsPos { lat_deg: -27.4698, lon_deg: 153.0251, elevation_m: 500.0 },
        look_at: GpsPos { lat_deg: -27.4758, lon_deg: 153.0311, elevation_m: 0.0 },
        description: "Aerial view 500m",
    },
    TestView {
        name: "07_tilted_down_200m",
        position: GpsPos { lat_deg: -27.4698, lon_deg: 153.0251, elevation_m: 200.0 },
        look_at: GpsPos { lat_deg: -27.4698, lon_deg: 153.0251, elevation_m: 0.0 },
        description: "200m looking straight down",
    },
    TestView {
        name: "08_river_view_20m",
        position: GpsPos { lat_deg: -27.4748, lon_deg: 153.0301, elevation_m: 20.0 },
        look_at: GpsPos { lat_deg: -27.4738, lon_deg: 153.0291, elevation_m: 5.0 },
        description: "River view from 20m",
    },
    TestView {
        name: "09_diagonal_150m",
        position: GpsPos { lat_deg: -27.4668, lon_deg: 153.0221, elevation_m: 150.0 },
        look_at: GpsPos { lat_deg: -27.4728, lon_deg: 153.0281, elevation_m: 20.0 },
        description: "Diagonal aerial view",
    },
    TestView {
        name: "10_low_oblique_80m",
        position: GpsPos { lat_deg: -27.4678, lon_deg: 153.0231, elevation_m: 80.0 },
        look_at: GpsPos { lat_deg: -27.4718, lon_deg: 153.0291, elevation_m: 30.0 },
        description: "Low oblique angle",
    },
];

fn main() {
    println!("=== AUTOMATED SCREENSHOT TEST ===");
    println!("Taking 10 screenshots from different angles");
    println!("Check screenshot/ folder for results\n");

    // Load OSM data
    let cache = DiskCache::new().unwrap();
    let osm_data = cache.load_osm("brisbane_cbd").ok();
    
    if osm_data.is_none() {
        println!("ERROR: No cached OSM data. Run download_brisbane_data first.");
        return;
    }
    
    let osm_data = osm_data.unwrap();
    println!("Loaded {} buildings, {} roads, {} water",
        osm_data.buildings.len(), osm_data.roads.len(), osm_data.water.len());

    // Generate mesh once
    println!("\nGenerating mesh...");
    let (vertices, indices) = generate_mesh_from_osm(&osm_data);
    println!("Generated {} vertices, {} indices\n", vertices.len(), indices.len());

    // Create headless renderer (offscreen)
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..Default::default()
    });

    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .unwrap();

    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: None,
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
        },
        None,
    ))
    .unwrap();

    println!("GPU device initialized\n");

    // Create buffers
    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Vertex Buffer"),
        contents: bytemuck::cast_slice(&vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });

    let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Index Buffer"),
        contents: bytemuck::cast_slice(&indices),
        usage: wgpu::BufferUsages::INDEX,
    });

    // Take screenshots from each test view
    for (i, view) in TEST_VIEWS.iter().enumerate() {
        println!("[{}/10] {}", i + 1, view.name);
        println!("  Pos: ({:.4}, {:.4}, {:.1}m)", 
            view.position.lat_deg, view.position.lon_deg, view.position.elevation_m);
        println!("  Look: ({:.4}, {:.4}, {:.1}m)",
            view.look_at.lat_deg, view.look_at.lon_deg, view.look_at.elevation_m);
        println!("  Description: {}", view.description);

        // TODO: Actually render and save screenshot
        // This would require implementing offscreen rendering
        // For now, just document what should be done
        
        println!("  [NOT YET IMPLEMENTED - needs offscreen render target]\n");
    }

    println!("\n=== TEST COMPLETE ===");
    println!("TODO: Implement actual screenshot capture");
    println!("Alternative: Manually fly to these positions and screenshot");
}
