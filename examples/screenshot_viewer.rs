use metaverse_core::coordinates::*;
use metaverse_core::renderer::Renderer;
use metaverse_core::world_manager::WorldManager;
use metaverse_core::camera::Camera;
use winit::{
    event_loop::EventLoop,
    window::WindowBuilder,
};
use std::sync::Arc;
use pollster::block_on;

fn main() {
    env_logger::init();
    
    println!("[screenshot_viewer] Starting...");
    
    // Create window and event loop
    let event_loop = EventLoop::new().unwrap();
    let window = WindowBuilder::new()
        .with_title("Screenshot Viewer")
        .with_inner_size(winit::dpi::PhysicalSize::new(1920, 1080))
        .build(&event_loop)
        .unwrap();
    let window = Arc::new(window);
    
    println!("[screenshot_viewer] Creating renderer...");
    let mut renderer = block_on(Renderer::new(window.clone())).unwrap();
    
    // Brisbane test position
    let brisbane_gps = GPSCoordinate::new(-27.4705, 153.0260);
    println!("[screenshot_viewer] Brisbane GPS: {:?}", brisbane_gps);
    
    let brisbane_ecef = gps_to_ecef(&brisbane_gps);
    println!("[screenshot_viewer] Brisbane ECEF: {:?}", brisbane_ecef);
    
    // Camera 500m above Brisbane, looking down
    let camera_pos = ECEFCoordinate {
        x: brisbane_ecef.x,
        y: brisbane_ecef.y,
        z: brisbane_ecef.z,
    };
    
    // Get "up" vector (away from Earth center)
    let up_x = camera_pos.x / camera_pos.distance_to_origin();
    let up_y = camera_pos.y / camera_pos.distance_to_origin();
    let up_z = camera_pos.z / camera_pos.distance_to_origin();
    
    // Move camera 500m "up" (away from center)
    let camera_pos = ECEFCoordinate {
        x: camera_pos.x + up_x * 500.0,
        y: camera_pos.y + up_y * 500.0,
        z: camera_pos.z + up_z * 500.0,
    };
    
    println!("[screenshot_viewer] Camera ECEF: {:?}", camera_pos);
    
    // Calculate look direction: from camera toward Brisbane center
    let look_x = brisbane_ecef.x - camera_pos.x;
    let look_y = brisbane_ecef.y - camera_pos.y;
    let look_z = brisbane_ecef.z - camera_pos.z;
    let look_len = (look_x * look_x + look_y * look_y + look_z * look_z).sqrt();
    
    let yaw = look_y.atan2(look_x);
    let pitch = -(look_z / look_len).asin();
    
    println!("[screenshot_viewer] Camera yaw: {:.2}°, pitch: {:.2}°", 
             yaw.to_degrees(), pitch.to_degrees());
    
    let mut camera = Camera::new(
        camera_pos,
        yaw,
        pitch,
        window.inner_size().width as f32 / window.inner_size().height as f32,
    );
    
    // Create WorldManager with correct settings
    println!("[screenshot_viewer] Creating WorldManager...");
    let mut world_manager = WorldManager::new(
        14, // chunk depth (400m chunks)
        2000.0, // render distance (2km)
        7, // SVO depth (128^3)
    );
    
    // Generate chunks around camera
    println!("[screenshot_viewer] Updating world chunks...");
    world_manager.update_world_chunks(&camera_pos);
    
    println!("[screenshot_viewer] Active chunks: {}", world_manager.get_active_chunks().len());
    
    // Render frame
    println!("[screenshot_viewer] Rendering frame...");
    match renderer.render(&camera, &world_manager) {
        Ok(_) => {
            println!("[screenshot_viewer] ✓ Frame rendered successfully");
        }
        Err(e) => {
            eprintln!("[screenshot_viewer] ✗ Render failed: {:?}", e);
        }
    }
    
    // Save screenshot
    println!("[screenshot_viewer] Capturing screenshot...");
    if let Err(e) = save_screenshot(&renderer, "screenshot/viewer_test.png") {
        eprintln!("[screenshot_viewer] ✗ Screenshot failed: {:?}", e);
    } else {
        println!("[screenshot_viewer] ✓ Screenshot saved to screenshot/viewer_test.png");
    }
    
    println!("[screenshot_viewer] Done.");
}

fn save_screenshot(renderer: &Renderer, path: &str) -> Result<(), Box<dyn std::error::Error>> {
    use std::path::Path;
    
    // Create screenshot directory if needed
    if let Some(parent) = Path::new(path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    
    // Get the texture from the renderer
    let (texture, width, height) = renderer.capture_frame()?;
    
    // Save as PNG
    image::save_buffer(
        path,
        &texture,
        width,
        height,
        image::ColorType::Rgba8,
    )?;
    
    Ok(())
}
