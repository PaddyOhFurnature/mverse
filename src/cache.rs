/// Disk cache for OSM and SRTM data.
/// 
/// Cache structure:
/// ```
/// ~/.metaverse/cache/
///   osm/
///     bbox_S27.475_W153.020_N27.465_E153.035.json
///   srtm/
///     N37W122.hgt
/// ```

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

/// Cache for OSM and SRTM data stored in ~/.metaverse/cache/
pub struct DiskCache {
    root: PathBuf,
}

impl DiskCache {
    /// Creates a new cache with default root (~/.metaverse/cache/)
    pub fn new() -> io::Result<Self> {
        let home = dirs::home_dir()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Home directory not found"))?;
        let root = home.join(".metaverse").join("cache");
        Ok(Self { root })
    }

    /// Creates a cache with custom root (for testing)
    pub fn with_root(root: PathBuf) -> Self {
        Self { root }
    }

    /// Reads OSM data from cache
    pub fn read_osm(&self, key: &str) -> io::Result<Vec<u8>> {
        let path = self.root.join("osm").join(format!("{}.json", key));
        let mut file = File::open(path)?;
        let mut data = Vec::new();
        file.read_to_end(&mut data)?;
        Ok(data)
    }

    /// Writes OSM data to cache (creates directories if needed)
    pub fn write_osm(&self, key: &str, data: &[u8]) -> io::Result<()> {
        let dir = self.root.join("osm");
        fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{}.json", key));
        let mut file = File::create(path)?;
        file.write_all(data)?;
        file.sync_all()?;
        Ok(())
    }

    /// Reads SRTM data from cache
    pub fn read_srtm(&self, tile_name: &str) -> io::Result<Vec<u8>> {
        let path = self.root.join("srtm").join(format!("{}.hgt", tile_name));
        let mut file = File::open(path)?;
        let mut data = Vec::new();
        file.read_to_end(&mut data)?;
        Ok(data)
    }

    /// Writes SRTM data to cache (creates directories if needed)
    pub fn write_srtm(&self, tile_name: &str, data: &[u8]) -> io::Result<()> {
        let dir = self.root.join("srtm");
        fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{}.hgt", tile_name));
        let mut file = File::create(path)?;
        file.write_all(data)?;
        file.sync_all()?;
        Ok(())
    }

    /// Returns the cache root directory
    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl Default for DiskCache {
    fn default() -> Self {
        Self::new().expect("Failed to create default cache")
    }
}
