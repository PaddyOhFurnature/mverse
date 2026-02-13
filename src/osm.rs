/// OpenStreetMap data fetching via Overpass API.

use std::sync::Mutex;
use std::time::{Duration, Instant};
use std::thread;

/// Client for querying OpenStreetMap data via Overpass API
pub struct OverpassClient {
    last_request: Mutex<Option<Instant>>,
    min_cooldown: Duration,
    endpoint: String,
}

impl OverpassClient {
    /// Creates a new client with specified cooldown in seconds (default endpoint)
    pub fn new(cooldown_seconds: u64) -> Self {
        Self {
            last_request: Mutex::new(None),
            min_cooldown: Duration::from_secs(cooldown_seconds),
            endpoint: "https://overpass-api.de/api/interpreter".to_string(),
        }
    }

    /// Creates a client with custom endpoint (for testing)
    pub fn with_endpoint(cooldown_seconds: u64, endpoint: String) -> Self {
        Self {
            last_request: Mutex::new(None),
            min_cooldown: Duration::from_secs(cooldown_seconds),
            endpoint,
        }
    }

    /// Queries a bounding box for buildings, roads, water, and parks
    pub fn query_bbox(
        &self,
        south: f64,
        west: f64,
        north: f64,
        east: f64,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let query = Self::build_query(south, west, north, east);
        self.execute_with_retry(&query, 5)
    }

    /// Builds Overpass QL query for a bounding box
    pub fn build_query(south: f64, west: f64, north: f64, east: f64) -> String {
        format!(
            "[out:json][timeout:25];\n\
             (\n  \
               way[\"building\"]({},{},{},{});\n  \
               way[\"highway\"]({},{},{},{});\n  \
               way[\"natural\"=\"water\"]({},{},{},{});\n  \
               way[\"leisure\"=\"park\"]({},{},{},{});\n  \
               relation[\"natural\"=\"water\"]({},{},{},{});\n\
             );\n\
             out body;\n\
             >;\n\
             out skel qt;",
            south, west, north, east, // building
            south, west, north, east, // highway
            south, west, north, east, // water way
            south, west, north, east, // park
            south, west, north, east, // water relation
        )
    }

    /// Executes query with exponential backoff retry
    fn execute_with_retry(
        &self,
        query: &str,
        max_retries: u32,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let mut attempt = 0;
        let mut backoff = Duration::from_secs(3);

        loop {
            // Enforce rate limiting
            self.wait_for_cooldown();

            // Execute request
            let client = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(30))
                .user_agent("metaverse-core/0.1 (Earth metaverse project)")
                .build()?;

            match client.post(&self.endpoint).body(query.to_string()).send() {
                Ok(response) => {
                    if response.status().is_success() {
                        let json = response.json()?;
                        return Ok(json);
                    } else if response.status() == 429 {
                        // Too Many Requests - exponential backoff
                        if attempt >= max_retries {
                            return Err(format!("Max retries exceeded (429 errors)").into());
                        }
                        thread::sleep(backoff);
                        backoff = std::cmp::min(backoff * 2, Duration::from_secs(60));
                        attempt += 1;
                    } else {
                        return Err(format!("HTTP error: {}", response.status()).into());
                    }
                }
                Err(e) => {
                    // Network error - retry with backoff
                    if attempt >= max_retries {
                        return Err(format!("Max retries exceeded: {}", e).into());
                    }
                    thread::sleep(backoff);
                    backoff = std::cmp::min(backoff * 2, Duration::from_secs(60));
                    attempt += 1;
                }
            }
        }
    }

    /// Waits for cooldown period if needed (public for testing)
    pub fn wait_for_cooldown(&self) {
        let mut last = self.last_request.lock().unwrap();
        if let Some(instant) = *last {
            let elapsed = instant.elapsed();
            if elapsed < self.min_cooldown {
                let wait = self.min_cooldown - elapsed;
                thread::sleep(wait);
            }
        }
        *last = Some(Instant::now());
    }

    /// Returns the backoff duration for a given attempt (for testing)
    pub fn backoff_duration(attempt: u32) -> Duration {
        let seconds = 3u64 * 2u64.pow(attempt);
        let capped = seconds.min(60);
        Duration::from_secs(capped)
    }
}
