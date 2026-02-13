// Coordinate system conversions: GPS ↔ ECEF ↔ ENU
// WGS84 ellipsoid model for geodetic calculations

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
