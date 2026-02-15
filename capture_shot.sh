#!/bin/bash
# Args: lat lon alt heading tilt filename
LAT=$1
LON=$2
ALT=$3
HEADING=$4
TILT=$5
OUTPUT=$6

# Create temp viewer with fixed camera
cat > /tmp/viewer_temp.rs << INNEREOF
// Temporary viewer for screenshot at position
use metaverse_core::*;
use winit::event::{Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::WindowBuilder;
use winit::dpi::PhysicalSize;
use std::sync::Arc;

fn main() {
    let lat = $LAT;
    let lon = $LON;
    let alt = $ALT;
    let heading = $HEADING;
    let tilt = $TILT;
    
    // Camera position at specified coords
    let camera_gps = GpsPos { lat, lon, alt };
    let camera_ecef = gps_to_ecef(&camera_gps);
    let camera_pos = glam::DVec3::new(camera_ecef.x, camera_ecef.y, camera_ecef.z);
    
    // Look direction from heading/tilt
    let heading_rad = heading.to_radians();
    let tilt_rad = (90.0 - tilt).to_radians();
    
    let dir_x = tilt_rad.sin() * heading_rad.sin();
    let dir_y = tilt_rad.sin() * heading_rad.cos();
    let dir_z = tilt_rad.cos();
    
    let look_at = camera_pos + glam::DVec3::new(dir_x, dir_y, dir_z) * 100.0;
    
    let camera = Camera::new(camera_pos, look_at);
    
    let event_loop = EventLoop::new().unwrap();
    let window = WindowBuilder::new()
        .with_inner_size(PhysicalSize::new(1920, 1080))
        .with_title("Screenshot")
        .build(&event_loop)
        .unwrap();
    
    // Render one frame then exit
    let mut frame = 0;
    event_loop.run(move |event, elwt| {
        match event {
            Event::WindowEvent { event: WindowEvent::RedrawRequested, .. } => {
                frame += 1;
                if frame > 3 {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    elwt.exit();
                }
            }
            _ => {}
        }
    }).unwrap();
}
INNEREOF

# Build and run with xvfb
Xvfb :99 -screen 0 1920x1080x24 &
XPID=$!
sleep 1
DISPLAY=:99 cargo run --release --bin viewer_temp 2>&1 | head -20 &
VPID=$!
sleep 3
DISPLAY=:99 import -window root "$OUTPUT"
kill $VPID $XPID 2>/dev/null
