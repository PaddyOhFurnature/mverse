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
#[derive(Clone)]
pub struct DiskCache {
    root: PathBuf,
}

impl DiskCache {
    /// Creates a new cache with default root inside the project directory
    ///
    /// Priority order:
    /// 1. METAVERSE_CACHE_DIR environment variable (if set)
    /// 2. ./ .metaverse/cache in current working directory (project-local)
    /// 3. ~/.metaverse/cache (legacy) - migrated into project-local if present
    pub fn new() -> io::Result<Self> {
        use std::env;

        // 1) Environment override
        if let Ok(dir) = env::var("METAVERSE_CACHE_DIR") {
            let root = PathBuf::from(dir);
            fs::create_dir_all(&root)?;
            return Ok(Self { root });
        }

        // 2) Project-local cache: ./ .metaverse/cache
        let cwd = env::current_dir()?;
        let project_root = cwd.join(".metaverse").join("cache");

        // If project-local cache doesn't exist but legacy home cache does, migrate it
        if !project_root.exists() {
            if let Some(home) = dirs::home_dir() {
                let legacy = home.join(".metaverse").join("cache");
                if legacy.exists() {
                    // Ensure parent dir exists
                    if let Some(parent) = project_root.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    // Attempt to move legacy cache into project-local cache
                    // If rename fails, fall back to copying
                    if fs::rename(&legacy, &project_root).is_err() {
                        // fallback: copy directory contents
                        fs::create_dir_all(&project_root)?;
                        // Attempt to copy recursively (best-effort)
                        for entry in fs::read_dir(&legacy)? {
                            let entry = entry?;
                            let dest = project_root.join(entry.file_name());
                            let _ = fs::rename(entry.path(), dest);
                        }
                    }
                } else {
                    fs::create_dir_all(&project_root)?;
                }
            } else {
                fs::create_dir_all(&project_root)?;
            }
        }

        Ok(Self { root: project_root })
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
        // Don't add .hgt extension if already present
        let filename = if tile_name.ends_with(".hgt") {
            tile_name.to_string()
        } else {
            format!("{}.hgt", tile_name)
        };
        
        let path = self.root.join("srtm").join(filename);
        let mut file = File::open(path)?;
        let mut data = Vec::new();
        file.read_to_end(&mut data)?;
        Ok(data)
    }

    /// Writes SRTM data to cache (creates directories if needed)
    pub fn write_srtm(&self, tile_name: &str, data: &[u8]) -> io::Result<()> {
        let dir = self.root.join("srtm");
        fs::create_dir_all(&dir)?;
        
        // Don't add .hgt extension if already present
        let filename = if tile_name.ends_with(".hgt") {
            tile_name.to_string()
        } else {
            format!("{}.hgt", tile_name)
        };
        
        let path = dir.join(filename);
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
