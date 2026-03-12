//! Configuration structs and serialization.
//!
//! UsageData stores opacity per hour. Keys are hour strings ("0".."23");
//! values are opacity floats. Serde derives handle JSON (de)serialization.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Opacity values per hour of the day.
///
/// Keys: hour as string ("0".."23"). Values: opacity (0.0..1.0).
/// Serialize/Deserialize derive enables JSON round-trip via serde_json.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct UsageData {
    /// Map from hour string to opacity. Uses HashMap for flexible JSON shape.
    #[serde(flatten)]
    pub by_hour: HashMap<String, f32>,
}

impl UsageData {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get opacity for the given hour, or None if not stored.
    pub fn get(&self, hour: u8) -> Option<f32> {
        self.by_hour.get(&hour.to_string()).copied()
    }

    /// Set opacity for the given hour.
    pub fn set(&mut self, hour: u8, opacity: f32) {
        self.by_hour.insert(hour.to_string(), opacity);
    }
}
