use crate::cache::DiskCache;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_write_and_read_osm() {
    let temp = TempDir::new().unwrap();
    let cache = DiskCache::with_root(temp.path().to_path_buf());
    
    let key = "bbox_S27.475_W153.020_N27.465_E153.035";
    let data = b"{\"elements\":[]}";
    
    cache.write_osm(key, data).unwrap();
    let read_data = cache.read_osm(key).unwrap();
    
    assert_eq!(data.as_slice(), read_data.as_slice());
}

#[test]
fn test_write_and_read_srtm() {
    let temp = TempDir::new().unwrap();
    let cache = DiskCache::with_root(temp.path().to_path_buf());
    
    let tile = "N37W122";
    let data = vec![0u8; 1024]; // Dummy elevation data
    
    cache.write_srtm(tile, &data).unwrap();
    let read_data = cache.read_srtm(tile).unwrap();
    
    assert_eq!(data, read_data);
}

#[test]
fn test_read_nonexistent_osm_returns_error() {
    let temp = TempDir::new().unwrap();
    let cache = DiskCache::with_root(temp.path().to_path_buf());
    
    let result = cache.read_osm("nonexistent");
    assert!(result.is_err());
}

#[test]
fn test_read_nonexistent_srtm_returns_error() {
    let temp = TempDir::new().unwrap();
    let cache = DiskCache::with_root(temp.path().to_path_buf());
    
    let result = cache.read_srtm("N99W999");
    assert!(result.is_err());
}

#[test]
fn test_write_creates_directories() {
    let temp = TempDir::new().unwrap();
    let cache = DiskCache::with_root(temp.path().to_path_buf());
    
    // Verify directories don't exist initially
    assert!(!temp.path().join("osm").exists());
    assert!(!temp.path().join("srtm").exists());
    
    // Write should create directories
    cache.write_osm("test", b"data").unwrap();
    cache.write_srtm("test", b"data").unwrap();
    
    assert!(temp.path().join("osm").exists());
    assert!(temp.path().join("srtm").exists());
    assert!(temp.path().join("osm").join("test.json").exists());
    assert!(temp.path().join("srtm").join("test.hgt").exists());
}

#[test]
fn test_cache_root_path() {
    let temp = TempDir::new().unwrap();
    let cache = DiskCache::with_root(temp.path().to_path_buf());
    
    assert_eq!(cache.root(), temp.path());
}
