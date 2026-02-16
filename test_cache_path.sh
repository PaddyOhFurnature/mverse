#!/bin/bash
# Test what path the cache generates
cd /home/main/metaverse/metaverse_core
cat > /tmp/test_path.rs << 'EORUST'
fn main() {
    let lat: i16 = -28;
    let lon: i16 = 153;
    let lat_prefix = if lat >= 0 { "N" } else { "S" };
    let lon_prefix = if lon >= 0 { "E" } else { "W" };
    let filename = format!(
        "{}{:02}{}{:03}.hgt",
        lat_prefix, lat.abs(),
        lon_prefix, lon.abs()
    );
    println!("Generated filename: {}", filename);
    println!("Expected filename: S28E153.hgt");
}
EORUST
rustc /tmp/test_path.rs -o /tmp/test_path && /tmp/test_path
