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
