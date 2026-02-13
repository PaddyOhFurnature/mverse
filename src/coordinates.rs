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
