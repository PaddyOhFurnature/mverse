use metaverse_core::cache::DiskCache;
use metaverse_core::elevation_downloader::ElevationDownloader;

fn main() {
    let cache = DiskCache::new().unwrap();
    let mut downloader = ElevationDownloader::new(cache);
    
    // Brisbane coordinates
    let lat = -27.4698;
    let lon = 153.0251;
    
    println!("Testing elevation at Brisbane ({}, {})", lat, lon);
    
    // Queue and download
    downloader.queue_download(lat, lon, 10, 0.0);
    
    // Process until done
    while downloader.get_stats().active_downloads > 0 || 
          downloader.get_stats().queued_downloads > 0 {
        downloader.process_queue();
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    
    let stats = downloader.get_stats();
    println!("Downloaded: {} success, {} failed", stats.downloads_success, stats.downloads_failed);
    
    // Now query elevation
    let elev = downloader.get_elevation(lat, lon, 10);
    println!("Elevation at Brisbane: {:?}", elev);
    
    // Try a few more points nearby
    for offset in [-0.1, 0.0, 0.1] {
        let e = downloader.get_elevation(lat + offset, lon, 10);
        println!("  Lat {} + {}: {:?}", lat, offset, e);
    }
}
