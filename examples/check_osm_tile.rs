fn check_tile(dir: &str, tile: &str, test_pts: &[(f64, f64, &str)]) {
    let path = format!("{}/{}", dir, tile);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(_) => {
            println!("{}: MISSING", tile);
            return;
        }
    };
    if bytes.len() < 4 {
        return;
    }
    let data: metaverse_core::osm::OsmData = match bincode::deserialize(&bytes[4..]) {
        Ok(d) => d,
        Err(e) => {
            println!("{}: ERR {}", tile, e);
            return;
        }
    };
    println!(
        "--- {} : buildings={} water={} wlines={} ---",
        tile,
        data.buildings.len(),
        data.water.len(),
        data.waterway_lines.len()
    );
    for (i, line) in data.waterway_lines.iter().enumerate() {
        let lats: Vec<f64> = line.iter().map(|p| p.lat).collect();
        let lons: Vec<f64> = line.iter().map(|p| p.lon).collect();
        let lat_min = lats.iter().cloned().fold(f64::INFINITY, f64::min);
        let lat_max = lats.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let lon_min = lons.iter().cloned().fold(f64::INFINITY, f64::min);
        let lon_max = lons.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let hits: Vec<&str> = test_pts
            .iter()
            .filter_map(|(lat, lon, label)| {
                if metaverse_core::osm::point_near_waterway_line(*lat, *lon, line, 0.002) {
                    Some(*label)
                } else {
                    None
                }
            })
            .collect();
        println!(
            "  {} centerline[{}] pts={} bbox=[{:.4}..{:.4}][{:.4}..{:.4}] hits={:?}",
            tile,
            i,
            line.len(),
            lat_min,
            lat_max,
            lon_min,
            lon_max,
            hits
        );
    }
    for (i, w) in data.water.iter().enumerate() {
        let lats: Vec<f64> = w.polygon.iter().map(|p| p.lat).collect();
        let lons: Vec<f64> = w.polygon.iter().map(|p| p.lon).collect();
        let lat_min = lats.iter().cloned().fold(f64::INFINITY, f64::min);
        let lat_max = lats.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let lon_min = lons.iter().cloned().fold(f64::INFINITY, f64::min);
        let lon_max = lons.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let hits: Vec<&str> = test_pts
            .iter()
            .filter_map(|(lat, lon, label)| {
                if metaverse_core::osm::point_in_polygon(*lat, *lon, &w.polygon) {
                    Some(*label)
                } else {
                    None
                }
            })
            .collect();
        if !hits.is_empty() {
            println!(
                "  *** {} water[{}] pts={} bbox=[{:.4}..{:.4}][{:.4}..{:.4}] HIT={:?}",
                tile,
                i,
                w.polygon.len(),
                lat_min,
                lat_max,
                lon_min,
                lon_max,
                hits
            );
        }
    }
}
fn main() {
    let dir = "world_data/osm";
    let pts: Vec<(f64, f64, &str)> = vec![
        (-27.470, 153.030, "A"),
        (-27.470, 153.035, "B"),
        (-27.470, 153.040, "C"),
        (-27.465, 153.030, "E"),
        (-27.465, 153.035, "F"),
        (-27.465, 153.040, "G"),
        (-27.462, 153.030, "I"),
        (-27.462, 153.035, "J"),
        (-27.462, 153.040, "K"),
        (-27.460, 153.030, "M"),
        (-27.460, 153.035, "N"),
        (-27.460, 153.040, "O"),
        (-27.458, 153.030, "Q"),
        (-27.458, 153.035, "R"),
        (-27.458, 153.040, "S"),
        (-27.455, 153.030, "U"),
        (-27.455, 153.035, "V"),
        (-27.455, 153.040, "W"),
    ];
    // Primary tile and all 4 neighbours for the -27.46 tile
    for t in &[
        "osm_-27.4600_153.0300_-27.4500_153.0400.bin",
        "osm_-27.4700_153.0300_-27.4600_153.0400.bin",
        "osm_-27.4500_153.0300_-27.4400_153.0400.bin",
        "osm_-27.4600_153.0200_-27.4500_153.0300.bin",
        "osm_-27.4600_153.0400_-27.4500_153.0500.bin",
    ] {
        check_tile(dir, t, &pts);
    }
}
