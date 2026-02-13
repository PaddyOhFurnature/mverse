// Coordinate system conversions: GPS ↔ ECEF ↔ ENU
// WGS84 ellipsoid model for geodetic calculations

use rayon::prelude::*;

/// WGS84 semi-major axis (equatorial radius) in metres
pub const WGS84_A: f64 = 6_378_137.0;

/// WGS84 flattening factor
pub const WGS84_F: f64 = 1.0 / 298.257_223_563;

/// WGS84 first eccentricity squared: e² = f × (2 - f)
pub const WGS84_E2: f64 = WGS84_F * (2.0 - WGS84_F);

/// WGS84 semi-minor axis (polar radius) in metres: b = a × (1 - f)
pub const WGS84_B: f64 = WGS84_A * (1.0 - WGS84_F);

/// GPS position in geodetic coordinates (WGS84)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GpsPos {
    /// Latitude in degrees (−90 to +90)
    pub lat_deg: f64,
    /// Longitude in degrees (−180 to +180)
    pub lon_deg: f64,
    /// Elevation above WGS84 ellipsoid in metres
    pub elevation_m: f64,
}

/// Position in Earth-Centered, Earth-Fixed (ECEF) coordinates
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EcefPos {
    /// X coordinate in metres (through equator at 0° longitude)
    pub x: f64,
    /// Y coordinate in metres (through equator at 90° East)
    pub y: f64,
    /// Z coordinate in metres (through North Pole)
    pub z: f64,
}

/// Position in East-North-Up (ENU) local tangent plane coordinates
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnuPos {
    /// East coordinate in metres (local tangent east direction)
    pub east: f64,
    /// North coordinate in metres (local tangent north direction)
    pub north: f64,
    /// Up coordinate in metres (local vertical, away from Earth centre)
    pub up: f64,
}

/// Converts a GPS position to ECEF (Earth-Centered Earth-Fixed) coordinates.
///
/// Uses the WGS84 ellipsoid model. The resulting ECEF position is in metres,
/// with origin at Earth's centre.
///
/// # Arguments
/// * `gps` - GPS position with latitude/longitude in degrees and elevation in metres
///
/// # Returns
/// ECEF position in metres
///
/// # Examples
/// ```
/// use metaverse_core::coordinates::{gps_to_ecef, GpsPos};
/// let gps = GpsPos { lat_deg: -27.4698, lon_deg: 153.0251, elevation_m: 0.0 };
/// let ecef = gps_to_ecef(&gps);
/// // ecef.x ≈ -5046137, ecef.y ≈ 2568285, ecef.z ≈ -2924797
/// ```
pub fn gps_to_ecef(gps: &GpsPos) -> EcefPos {
    // Convert degrees to radians
    let phi = gps.lat_deg.to_radians();
    let lambda = gps.lon_deg.to_radians();
    let h = gps.elevation_m;
    
    // Compute prime vertical radius of curvature N(φ)
    // N = a / sqrt(1 - e² × sin²(φ))
    let sin_phi = phi.sin();
    let n = WGS84_A / (1.0 - WGS84_E2 * sin_phi * sin_phi).sqrt();
    
    // Compute ECEF coordinates
    let cos_phi = phi.cos();
    let cos_lambda = lambda.cos();
    let sin_lambda = lambda.sin();
    
    let x = (n + h) * cos_phi * cos_lambda;
    let y = (n + h) * cos_phi * sin_lambda;
    let z = (n * (1.0 - WGS84_E2) + h) * sin_phi;
    
    EcefPos { x, y, z }
}

/// Converts ECEF coordinates to GPS position (geodetic coordinates).
///
/// Uses the iterative Bowring method for WGS84 ellipsoid. Converges to
/// sub-millimetre accuracy within a few iterations.
///
/// # Arguments
/// * `ecef` - ECEF position in metres
///
/// # Returns
/// GPS position with latitude/longitude in degrees and elevation in metres
///
/// # Examples
/// ```
/// use metaverse_core::coordinates::{ecef_to_gps, EcefPos};
/// let ecef = EcefPos { x: -5046951.809, y: 2568766.054, z: -2924501.502 };
/// let gps = ecef_to_gps(&ecef);
/// // gps.lat_deg ≈ -27.4698, gps.lon_deg ≈ 153.0251
/// ```
pub fn ecef_to_gps(ecef: &EcefPos) -> GpsPos {
    let x = ecef.x;
    let y = ecef.y;
    let z = ecef.z;
    
    // Compute longitude (straightforward)
    let lambda = y.atan2(x);
    
    // Compute latitude iteratively using Bowring's method
    let p = (x * x + y * y).sqrt();
    
    // Initial estimate for latitude
    let mut phi = (z / (p * (1.0 - WGS84_E2))).atan();
    
    // Iterate until convergence
    let mut phi_prev;
    let convergence_threshold = 1e-12; // radians (< 0.01mm)
    let max_iterations = 10;
    
    for _ in 0..max_iterations {
        phi_prev = phi;
        
        let sin_phi = phi.sin();
        let n = WGS84_A / (1.0 - WGS84_E2 * sin_phi * sin_phi).sqrt();
        
        phi = (z + WGS84_E2 * n * sin_phi).atan2(p);
        
        if (phi - phi_prev).abs() < convergence_threshold {
            break;
        }
    }
    
    // Compute elevation
    let sin_phi = phi.sin();
    let cos_phi = phi.cos();
    let n = WGS84_A / (1.0 - WGS84_E2 * sin_phi * sin_phi).sqrt();
    
    let h = if cos_phi.abs() > 1e-10 {
        p / cos_phi - n
    } else {
        // At poles, use z-axis calculation
        z.abs() / sin_phi.abs() - n * (1.0 - WGS84_E2)
    };
    
    GpsPos {
        lat_deg: phi.to_degrees(),
        lon_deg: lambda.to_degrees(),
        elevation_m: h,
    }
}

/// Calculates the great-circle distance between two GPS positions using the Haversine formula.
///
/// Returns the distance along the surface of the WGS84 ellipsoid (approximated as a sphere
/// with radius = WGS84_A). Ignores elevation differences.
///
/// # Arguments
/// * `a` - First GPS position
/// * `b` - Second GPS position
///
/// # Returns
/// Distance in metres along the great circle
///
/// # Examples
/// ```
/// use metaverse_core::coordinates::{haversine_distance, GpsPos};
/// let brisbane = GpsPos { lat_deg: -27.4698, lon_deg: 153.0251, elevation_m: 0.0 };
/// let sydney = GpsPos { lat_deg: -33.8688, lon_deg: 151.2093, elevation_m: 0.0 };
/// let distance = haversine_distance(&brisbane, &sydney);
/// // distance ≈ 732,000 metres
/// ```
pub fn haversine_distance(a: &GpsPos, b: &GpsPos) -> f64 {
    // Convert to radians
    let phi1 = a.lat_deg.to_radians();
    let phi2 = b.lat_deg.to_radians();
    let delta_phi = (b.lat_deg - a.lat_deg).to_radians();
    let delta_lambda = (b.lon_deg - a.lon_deg).to_radians();
    
    // Haversine formula
    let a_val = (delta_phi / 2.0).sin().powi(2)
        + phi1.cos() * phi2.cos() * (delta_lambda / 2.0).sin().powi(2);
    
    let c = 2.0 * a_val.sqrt().atan2((1.0 - a_val).sqrt());
    
    // Distance = radius × angular distance
    WGS84_A * c
}

/// Calculates the straight-line Euclidean distance between two ECEF positions.
///
/// This is the direct 3D distance through space (potentially through the Earth),
/// not the surface distance. Always shorter than the great-circle distance for
/// the same two points.
///
/// # Arguments
/// * `a` - First ECEF position
/// * `b` - Second ECEF position
///
/// # Returns
/// Distance in metres
///
/// # Examples
/// ```
/// use metaverse_core::coordinates::{ecef_distance, gps_to_ecef, GpsPos};
/// let brisbane = gps_to_ecef(&GpsPos { lat_deg: -27.4698, lon_deg: 153.0251, elevation_m: 0.0 });
/// let sydney = gps_to_ecef(&GpsPos { lat_deg: -33.8688, lon_deg: 151.2093, elevation_m: 0.0 });
/// let distance = ecef_distance(&brisbane, &sydney);
/// // Straight-line distance < surface distance
/// ```
pub fn ecef_distance(a: &EcefPos, b: &EcefPos) -> f64 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let dz = b.z - a.z;
    
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// Converts a batch of GPS positions to ECEF coordinates in parallel.
///
/// Uses Rayon for parallel processing to achieve high throughput on multi-core systems.
/// Results are bitwise identical to sequential `gps_to_ecef()` calls.
///
/// # Arguments
/// * `positions` - Slice of GPS positions to convert
///
/// # Returns
/// Vector of ECEF positions in the same order as input
///
/// # Performance
/// Target: >1M conversions/sec in release mode on mid-range hardware
///
/// # Examples
/// ```
/// use metaverse_core::coordinates::{gps_to_ecef_batch, GpsPos};
/// let positions = vec![
///     GpsPos { lat_deg: -27.4698, lon_deg: 153.0251, elevation_m: 0.0 },
///     GpsPos { lat_deg: -33.8688, lon_deg: 151.2093, elevation_m: 0.0 },
/// ];
/// let ecef_positions = gps_to_ecef_batch(&positions);
/// assert_eq!(ecef_positions.len(), 2);
/// ```
pub fn gps_to_ecef_batch(positions: &[GpsPos]) -> Vec<EcefPos> {
    positions.par_iter()
        .map(|pos| gps_to_ecef(pos))
        .collect()
}

/// Converts an ECEF position to local East-North-Up (ENU) coordinates.
///
/// ENU coordinates are relative to a local tangent plane at the origin point.
/// The origin is defined by its ECEF position and GPS coordinates (needed for latitude/longitude).
///
/// # Arguments
/// * `point` - ECEF position to convert
/// * `origin` - Origin point in ECEF coordinates
/// * `origin_gps` - Origin point in GPS coordinates (for lat/lon angles)
///
/// # Returns
/// ENU position relative to origin
///
/// # Examples
/// ```
/// use metaverse_core::coordinates::{ecef_to_enu, gps_to_ecef, GpsPos};
/// let origin_gps = GpsPos { lat_deg: -27.4698, lon_deg: 153.0251, elevation_m: 0.0 };
/// let origin_ecef = gps_to_ecef(&origin_gps);
/// let enu = ecef_to_enu(&origin_ecef, &origin_ecef, &origin_gps);
/// // Origin point in ENU frame is (0, 0, 0)
/// ```
pub fn ecef_to_enu(point: &EcefPos, origin: &EcefPos, origin_gps: &GpsPos) -> EnuPos {
    // Compute ECEF offset from origin
    let dx = point.x - origin.x;
    let dy = point.y - origin.y;
    let dz = point.z - origin.z;
    
    // Convert origin lat/lon to radians
    let phi = origin_gps.lat_deg.to_radians();
    let lambda = origin_gps.lon_deg.to_radians();
    
    let sin_phi = phi.sin();
    let cos_phi = phi.cos();
    let sin_lambda = lambda.sin();
    let cos_lambda = lambda.cos();
    
    // Rotation matrix from ECEF to ENU
    // ENU frame: East = -sin(λ), cos(λ), 0
    //            North = -sin(φ)cos(λ), -sin(φ)sin(λ), cos(φ)
    //            Up = cos(φ)cos(λ), cos(φ)sin(λ), sin(φ)
    
    let east = -sin_lambda * dx + cos_lambda * dy;
    let north = -sin_phi * cos_lambda * dx - sin_phi * sin_lambda * dy + cos_phi * dz;
    let up = cos_phi * cos_lambda * dx + cos_phi * sin_lambda * dy + sin_phi * dz;
    
    EnuPos { east, north, up }
}

/// Converts local East-North-Up (ENU) coordinates to ECEF position.
///
/// Inverse of `ecef_to_enu()`. Converts a point in local ENU frame back to
/// global ECEF coordinates.
///
/// # Arguments
/// * `enu` - ENU position relative to origin
/// * `origin` - Origin point in ECEF coordinates
/// * `origin_gps` - Origin point in GPS coordinates (for lat/lon angles)
///
/// # Returns
/// ECEF position
///
/// # Examples
/// ```
/// use metaverse_core::coordinates::{enu_to_ecef, EnuPos, gps_to_ecef, GpsPos};
/// let origin_gps = GpsPos { lat_deg: -27.4698, lon_deg: 153.0251, elevation_m: 0.0 };
/// let origin_ecef = gps_to_ecef(&origin_gps);
/// let enu = EnuPos { east: 100.0, north: 0.0, up: 0.0 };
/// let ecef = enu_to_ecef(&enu, &origin_ecef, &origin_gps);
/// // Point 100m east of origin in ECEF
/// ```
pub fn enu_to_ecef(enu: &EnuPos, origin: &EcefPos, origin_gps: &GpsPos) -> EcefPos {
    // Convert origin lat/lon to radians
    let phi = origin_gps.lat_deg.to_radians();
    let lambda = origin_gps.lon_deg.to_radians();
    
    let sin_phi = phi.sin();
    let cos_phi = phi.cos();
    let sin_lambda = lambda.sin();
    let cos_lambda = lambda.cos();
    
    // Rotation matrix from ENU to ECEF (transpose of ECEF to ENU)
    let dx = -sin_lambda * enu.east - sin_phi * cos_lambda * enu.north + cos_phi * cos_lambda * enu.up;
    let dy = cos_lambda * enu.east - sin_phi * sin_lambda * enu.north + cos_phi * sin_lambda * enu.up;
    let dz = cos_phi * enu.north + sin_phi * enu.up;
    
    EcefPos {
        x: origin.x + dx,
        y: origin.y + dy,
        z: origin.z + dz,
    }
}
