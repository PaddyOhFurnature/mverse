/// Asynchronous chunk loading subsystem
///
/// Loads chunks in background thread to avoid blocking main game loop.
/// Supports terrain generation, mesh generation, and collider generation.

use crate::chunk::ChunkId;
use crate::coordinates::ECEF;
use crate::voxel::Octree;
use std::sync::mpsc::{channel, Sender, Receiver};
use std::thread;
use std::time::Instant;

/// Request to load a chunk in the background
#[derive(Debug, Clone)]
pub struct LoadRequest {
    pub chunk_id: ChunkId,
    pub priority: f64,  // Distance from player (lower = higher priority)
}

/// Result of a chunk load operation
pub struct LoadResult {
    pub chunk_id: ChunkId,
    pub octree: Option<Octree>,
    pub load_time_ms: u128,
    pub error: Option<String>,
}

/// Commands sent to the background loader thread
enum LoaderCommand {
    Load(LoadRequest),
    Shutdown,
}

/// Background chunk loader
///
/// Runs in a separate thread, generating terrain/meshes asynchronously.
/// Main thread sends load requests, receives completed chunks via channels.
pub struct ChunkLoader {
    /// Send commands to background thread
    cmd_tx: Sender<LoaderCommand>,
    
    /// Receive completed chunks from background thread
    result_rx: Receiver<LoadResult>,
    
    /// Statistics
    pub chunks_loaded: usize,
    pub total_load_time_ms: u128,
}

impl ChunkLoader {
    /// Create new background chunk loader
    ///
    /// Spawns a background thread that processes load requests.
    pub fn new() -> Self {
        let (cmd_tx, cmd_rx) = channel();
        let (result_tx, result_rx) = channel();
        
        // Spawn background worker thread
        thread::spawn(move || {
            Self::worker_thread(cmd_rx, result_tx);
        });
        
        ChunkLoader {
            cmd_tx,
            result_rx,
            chunks_loaded: 0,
            total_load_time_ms: 0,
        }
    }
    
    /// Request a chunk to be loaded
    ///
    /// Non-blocking - adds to queue, returns immediately.
    pub fn request_load(&mut self, chunk_id: ChunkId, priority: f64) -> Result<(), String> {
        self.cmd_tx.send(LoaderCommand::Load(LoadRequest {
            chunk_id,
            priority,
        })).map_err(|e| format!("Failed to send load request: {}", e))
    }
    
    /// Poll for completed chunks (non-blocking)
    ///
    /// Returns all chunks that finished loading since last poll.
    pub fn poll_completed(&mut self) -> Vec<LoadResult> {
        let mut results = Vec::new();
        
        // Drain all available results
        while let Ok(result) = self.result_rx.try_recv() {
            self.chunks_loaded += 1;
            self.total_load_time_ms += result.load_time_ms;
            results.push(result);
        }
        
        results
    }
    
    /// Average load time per chunk (for performance monitoring)
    pub fn avg_load_time_ms(&self) -> f64 {
        if self.chunks_loaded == 0 {
            0.0
        } else {
            self.total_load_time_ms as f64 / self.chunks_loaded as f64
        }
    }
    
    /// Background worker thread
    ///
    /// Processes load requests, generates terrain, sends results back.
    fn worker_thread(cmd_rx: Receiver<LoaderCommand>, result_tx: Sender<LoadResult>) {
        // TODO: Initialize terrain generator once per thread
        // For now, just create empty octrees
        
        loop {
            match cmd_rx.recv() {
                Ok(LoaderCommand::Load(request)) => {
                    let start = Instant::now();
                    
                    // TODO: Generate terrain for this chunk
                    // For now, create empty octree as placeholder
                    let octree = Octree::new();
                    
                    let load_time_ms = start.elapsed().as_millis();
                    
                    let result = LoadResult {
                        chunk_id: request.chunk_id,
                        octree: Some(octree),
                        load_time_ms,
                        error: None,
                    };
                    
                    if result_tx.send(result).is_err() {
                        // Main thread dropped receiver, exit
                        break;
                    }
                }
                Ok(LoaderCommand::Shutdown) => {
                    break;
                }
                Err(_) => {
                    // Main thread dropped sender, exit
                    break;
                }
            }
        }
    }
    
    /// Shutdown background thread gracefully
    pub fn shutdown(&mut self) {
        let _ = self.cmd_tx.send(LoaderCommand::Shutdown);
    }
}

impl Drop for ChunkLoader {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;
    
    #[test]
    fn test_chunk_loader_basic() {
        let mut loader = ChunkLoader::new();
        
        // Request a chunk
        let chunk_id = ChunkId::new(0, 0, 0);
        loader.request_load(chunk_id, 0.0).unwrap();
        
        // Wait for completion (give background thread time to work)
        thread::sleep(Duration::from_millis(100));
        
        // Poll for results
        let results = loader.poll_completed();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].chunk_id, chunk_id);
        assert!(results[0].octree.is_some());
        assert!(results[0].error.is_none());
    }
    
    #[test]
    fn test_chunk_loader_multiple() {
        let mut loader = ChunkLoader::new();
        
        // Request multiple chunks
        for i in 0..5 {
            loader.request_load(ChunkId::new(i, 0, 0), i as f64).unwrap();
        }
        
        // Wait for completion
        thread::sleep(Duration::from_millis(200));
        
        // Poll for results
        let results = loader.poll_completed();
        assert_eq!(results.len(), 5);
        
        // All should succeed
        for result in &results {
            assert!(result.octree.is_some());
            assert!(result.error.is_none());
        }
    }
    
    #[test]
    fn test_chunk_loader_stats() {
        let mut loader = ChunkLoader::new();
        
        // Load some chunks
        for i in 0..3 {
            loader.request_load(ChunkId::new(i, 0, 0), 0.0).unwrap();
        }
        
        thread::sleep(Duration::from_millis(150));
        loader.poll_completed();
        
        // Check stats
        assert_eq!(loader.chunks_loaded, 3);
        assert!(loader.avg_load_time_ms() > 0.0);
    }
}
