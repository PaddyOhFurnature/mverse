//! Tier 3 biome and substrate classification.
//!
//! Pure deterministic classification — same inputs produce the same output on any machine
//! globally.  No random numbers are used anywhere in this module.
//!
//! # Overview
//! * [`koppen_climate`]        — Köppen climate zone from lat + elevation
//! * [`classify_biome`]        — Biome from climate, terrain metrics, and optional OSM tag
//! * [`classify_substrate`]    — Substrate / soil type from biome + terrain metrics
//! * [`classify_column`]       — Combined per-column output used by terrain voxelisation

use crate::terrain_analysis::TerrainAnalysis;

// ── Köppen climate ────────────────────────────────────────────────────────────

/// Köppen–Geiger climate classification, simplified to 15 zones.
///
/// Derived purely from latitude and elevation (no precipitation lookup required).
/// This is a deliberate approximation — globally reasonable, not locally perfect.
/// It will be refined when WorldClim raster data becomes available.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ClimateZone {
    TropicalRainforest, // Af  — equatorial, always wet
    TropicalMonsoon,    // Am  — equatorial, monsoon
    TropicalSavanna,    // Aw  — equatorial, dry winter
    HotDesert,          // BWh — hot arid
    ColdDesert,         // BWk — cold arid
    HotSteppe,          // BSh — hot semi-arid
    ColdSteppe,         // BSk — cold semi-arid
    HumidSubtropical,   // Cfa — warm temperate, no dry season, hot summer
    OceanicMaritime,    // Cfb — warm temperate, no dry season, warm summer
    MediterraneanHot,   // Csa — warm temperate, dry hot summer
    MediterraneanWarm,  // Csb — warm temperate, dry warm summer
    HumidContinental,   // Dfa/Dfb — snow, no dry season
    SubarcticBoreal,    // Dfc — subarctic
    Tundra,             // ET  — polar tundra
    IceCap,             // EF  — polar ice
}

/// Classify a location's Köppen climate zone from latitude and elevation alone.
///
/// Elevation adjusts the effective latitude using a standard lapse-rate approximation
/// (~6.5 °C per 1 000 m, encoded here as `elev_m / 150` equivalent-latitude degrees).
/// The steppe and desert variants are not currently distinguished by this function —
/// they are resolved by the biome classifier using TWI/moisture context.
pub fn koppen_climate(lat: f64, elevation_m: f32) -> ClimateZone {
    let abs_lat = lat.abs();
    let elev = elevation_m as f64;
    let effective_lat = abs_lat + elev / 150.0;

    match effective_lat as u32 {
        0..=10 => ClimateZone::TropicalRainforest,
        11..=20 => ClimateZone::TropicalMonsoon,
        21..=25 => ClimateZone::TropicalSavanna,
        26..=35 => ClimateZone::HumidSubtropical,
        36..=40 => ClimateZone::MediterraneanHot,
        41..=50 => ClimateZone::OceanicMaritime,
        51..=60 => ClimateZone::HumidContinental,
        61..=70 => ClimateZone::SubarcticBoreal,
        71..=80 => ClimateZone::Tundra,
        _ => ClimateZone::IceCap,
    }
}

// ── OSM landuse ───────────────────────────────────────────────────────────────

/// Minimal OSM `landuse=*` tag values used to override biome classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsmLanduse {
    Residential,
    Commercial,
    Industrial,
    Retail,
    Forest,
    Farmland,
    Meadow,
    Water,
}

// ── Biome ─────────────────────────────────────────────────────────────────────

/// Ecological biome classification for a single terrain column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Biome {
    // Water
    Ocean,
    Lake,
    River,
    Wetland,
    // Coastal
    Beach,
    MangroveCoast,
    RockyCoast,
    // Lowland subtropical (Brisbane region focus)
    SubtropicalForest,
    SubtropicalGrassland,
    RiparianCorridor,
    // Urban
    Urban,
    // Agricultural
    Agricultural,
    // Upland
    DrySclerophyllForest, // Australian dry eucalypt
    MontaneForest,
    AlpineGrassland,
    // Arid
    Shrubland,
    Desert,
    // Cold
    BorealForest,
    Tundra,
    IceCap,
}

/// Classify the biome for a terrain column from terrain metrics and optional OSM data.
///
/// OSM landuse tags take precedence over computed classification when present.
pub fn classify_biome(
    _lat: f64,
    _lon: f64,
    elevation_m: f32,
    slope_deg: f32,
    twi: f32,
    _tri: f32,
    coastal_dist_m: f32,
    osm_landuse: Option<OsmLanduse>,
    climate: ClimateZone,
) -> Biome {
    // OSM landuse overrides (most reliable ground-truth)
    if let Some(landuse) = osm_landuse {
        match landuse {
            OsmLanduse::Residential
            | OsmLanduse::Commercial
            | OsmLanduse::Industrial
            | OsmLanduse::Retail => return Biome::Urban,
            OsmLanduse::Forest => { /* fall through to elevation/climate check */ }
            OsmLanduse::Farmland | OsmLanduse::Meadow => return Biome::Agricultural,
            OsmLanduse::Water => return Biome::Lake,
        }
    }

    // Below sea level → ocean
    if elevation_m < 0.0 {
        return Biome::Ocean;
    }

    // Coastal zone (within 500 m of coast)
    if coastal_dist_m > 0.0 && coastal_dist_m < 500.0 {
        if slope_deg > 20.0 {
            return Biome::RockyCoast;
        }
        if twi > 10.0 {
            return Biome::MangroveCoast;
        }
        return Biome::Beach;
    }

    // Very wet low areas → wetland / riparian
    if twi > 14.0 {
        return Biome::Wetland;
    }
    if twi > 10.0 && slope_deg < 5.0 {
        return Biome::RiparianCorridor;
    }

    // Alpine above treeline (~varies by climate zone)
    let treeline_m: f32 = match climate {
        ClimateZone::TropicalRainforest | ClimateZone::TropicalMonsoon => 3500.0,
        ClimateZone::HumidSubtropical => 1800.0,
        ClimateZone::OceanicMaritime | ClimateZone::HumidContinental => 1200.0,
        ClimateZone::SubarcticBoreal => 600.0,
        ClimateZone::Tundra | ClimateZone::IceCap => 0.0,
        _ => 2000.0,
    };
    if elevation_m > treeline_m {
        return if elevation_m > treeline_m + 500.0 {
            Biome::IceCap
        } else {
            Biome::AlpineGrassland
        };
    }

    // Main classification by climate zone + terrain moisture
    match climate {
        ClimateZone::TropicalRainforest
        | ClimateZone::TropicalMonsoon
        | ClimateZone::TropicalSavanna => {
            if elevation_m > 800.0 {
                Biome::MontaneForest
            } else {
                Biome::SubtropicalForest
            }
        }
        ClimateZone::HumidSubtropical => {
            if elevation_m > 600.0 {
                Biome::DrySclerophyllForest
            } else if twi > 7.0 {
                Biome::SubtropicalForest
            } else {
                Biome::SubtropicalGrassland
            }
        }
        ClimateZone::MediterraneanHot | ClimateZone::MediterraneanWarm => {
            if twi > 7.0 {
                Biome::SubtropicalForest
            } else {
                Biome::Shrubland
            }
        }
        ClimateZone::OceanicMaritime | ClimateZone::HumidContinental => {
            Biome::SubtropicalForest // temperate broadleaf — reuse variant for now
        }
        ClimateZone::SubarcticBoreal => Biome::BorealForest,
        ClimateZone::HotDesert | ClimateZone::ColdDesert => Biome::Desert,
        ClimateZone::HotSteppe | ClimateZone::ColdSteppe => Biome::Shrubland,
        ClimateZone::Tundra => Biome::Tundra,
        ClimateZone::IceCap => Biome::IceCap,
    }
}

// ── Substrate ─────────────────────────────────────────────────────────────────

/// Soil / rock substrate underlying a terrain column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SubstrateType {
    BedRock,        // Exposed rock surface
    Sandstone,      // Sandy sedimentary rock (D'Aguilar range, Brisbane hills)
    Granite,        // Hard igneous (Wivenhoe catchment uplands)
    Basalt,         // Volcanic (some QLD highlands)
    RedClay,        // Iron-rich clay soil (western Brisbane suburbs)
    GreyClay,       // Alluvial clay (river flats, floodplains)
    Silty,          // River silt (lower river, estuarine)
    Sand,           // Coastal / fluvial sand
    GravelBed,      // River gravel (upland streams)
    Peat,           // Waterlogged organic (wetlands)
    UrbanFill,      // Made ground, concrete, asphalt
    TropicalRed,    // Laterite / red tropical soils
    TemperateBrown, // Brown forest soils (temperate)
    Permafrost,     // Frozen ground
}

/// Classify the substrate for a column from biome and terrain metrics.
pub fn classify_substrate(
    biome: Biome,
    climate: ClimateZone,
    slope_deg: f32,
    twi: f32,
    tri: f32,
    elevation_m: f32,
    _coastal_dist_m: f32,
) -> SubstrateType {
    if biome == Biome::Urban {
        return SubstrateType::UrbanFill;
    }

    if biome == Biome::Ocean || biome == Biome::Lake {
        return SubstrateType::Sand;
    }
    if biome == Biome::Wetland {
        return SubstrateType::Peat;
    }

    if biome == Biome::Beach {
        return SubstrateType::Sand;
    }
    if biome == Biome::MangroveCoast {
        return SubstrateType::Silty;
    }

    // Exposed bedrock: very steep + rugged
    if slope_deg > 50.0 && tri > 40.0 {
        return SubstrateType::BedRock;
    }

    // River corridor substrate depends on stream gradient (approximated by slope)
    if biome == Biome::RiparianCorridor || biome == Biome::River {
        return match slope_deg as u32 {
            0..=1 => SubstrateType::Silty,    // tidal / lower reach
            2..=4 => SubstrateType::GreyClay, // lowland
            _ => SubstrateType::GravelBed,    // upland / steep
        };
    }

    if biome == Biome::IceCap || biome == Biome::Tundra {
        return SubstrateType::Permafrost;
    }

    // Main classification by climate + elevation + moisture
    match climate {
        ClimateZone::TropicalRainforest
        | ClimateZone::TropicalMonsoon
        | ClimateZone::TropicalSavanna => SubstrateType::TropicalRed,

        ClimateZone::HumidSubtropical => {
            if elevation_m > 400.0 {
                SubstrateType::Sandstone
            } else if twi > 8.0 {
                SubstrateType::GreyClay
            } else {
                SubstrateType::RedClay
            }
        }

        ClimateZone::MediterraneanHot | ClimateZone::MediterraneanWarm => SubstrateType::Sandstone,

        ClimateZone::OceanicMaritime | ClimateZone::HumidContinental => {
            SubstrateType::TemperateBrown
        }

        ClimateZone::SubarcticBoreal => SubstrateType::TemperateBrown,

        ClimateZone::HotDesert | ClimateZone::ColdDesert => SubstrateType::Sand,

        ClimateZone::HotSteppe | ClimateZone::ColdSteppe => SubstrateType::RedClay,

        _ => SubstrateType::TemperateBrown,
    }
}

// ── ColumnClassification ──────────────────────────────────────────────────────

/// Complete per-column classification used by terrain voxelisation.
#[derive(Debug, Clone)]
pub struct ColumnClassification {
    pub biome: Biome,
    pub substrate: SubstrateType,
    pub climate: ClimateZone,
    /// How many voxels of soil overlie bedrock.
    pub soil_depth_voxels: u8,
    /// True when the surface is urban-sealed (road / pavement / roof).
    pub surface_is_sealed: bool,
}

/// Classify a terrain column, combining climate, biome, and substrate.
///
/// `analysis` is `None` when terrain analysis hasn't been computed yet;
/// moderate defaults are used in that case.
pub fn classify_column(
    lat: f64,
    lon: f64,
    elevation_m: f32,
    analysis: Option<&TerrainAnalysis>,
    osm_landuse: Option<OsmLanduse>,
    coastal_dist_m: f32,
) -> ColumnClassification {
    let (slope, twi, tri, _aspect) = if let Some(a) = analysis {
        (
            a.slope_at(lat, lon),
            a.twi_at(lat, lon),
            a.tri_at(lat, lon),
            a.aspect_at(lat, lon),
        )
    } else {
        (5.0_f32, 7.0_f32, 5.0_f32, 0.0_f32) // flat moderate defaults
    };

    let climate = koppen_climate(lat, elevation_m);
    let biome = classify_biome(
        lat,
        lon,
        elevation_m,
        slope,
        twi,
        tri,
        coastal_dist_m,
        osm_landuse,
        climate,
    );
    let substrate =
        classify_substrate(biome, climate, slope, twi, tri, elevation_m, coastal_dist_m);

    let soil_depth_voxels: u8 = match substrate {
        SubstrateType::BedRock => 0,
        SubstrateType::GravelBed => 1,
        SubstrateType::Granite => 2,
        SubstrateType::Basalt => 2,
        SubstrateType::Sandstone => 3,
        SubstrateType::Sand => 6,
        SubstrateType::TemperateBrown => 6,
        SubstrateType::RedClay | SubstrateType::GreyClay => 8,
        SubstrateType::TropicalRed => 10,
        SubstrateType::Silty | SubstrateType::Peat => 12,
        SubstrateType::UrbanFill => 0,
        SubstrateType::Permafrost => 0,
    };

    ColumnClassification {
        biome,
        substrate,
        climate,
        soil_depth_voxels,
        surface_is_sealed: matches!(substrate, SubstrateType::UrbanFill),
    }
}
