use crate::{errors::ApiError, services::activity_map::ActivityMapService, Host};
use sworm_protocol::activity_map::DiscoveredProject;

impl Host {
    /// Return the cached activity map, scanning on first call.
    pub fn activity_map_get(&self) -> Result<Vec<DiscoveredProject>, ApiError> {
        if let Some(cached) = &*self.activity_map_cache.lock() {
            return Ok(cached.clone());
        }

        self.activity_map_refresh()
    }

    /// Force rescan of all external agent history and return fresh results.
    pub fn activity_map_refresh(&self) -> Result<Vec<DiscoveredProject>, ApiError> {
        let results = ActivityMapService::scan();
        *self.activity_map_cache.lock() = Some(results.clone());
        Ok(results)
    }
}
