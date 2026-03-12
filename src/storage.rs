//! Read/write usage data to JSON file.
//!
//! File name: usage.json. Stored in the current working directory
//! (or app data dir if we add that later). Errors are logged but do not panic.

use crate::settings::UsageData;
use std::path::Path;

/// Default file name for usage persistence.
pub const USAGE_FILE: &str = "usage.json";

/// Load UsageData from usage.json. Returns default if file missing or invalid.
pub fn load(path: &Path) -> UsageData {
    match std::fs::read_to_string(path) {
        Ok(s) => match serde_json::from_str(&s) {
            Ok(data) => data,
            Err(_) => UsageData::new(),
        },
        Err(_) => UsageData::new(),
    }
}

/// Save UsageData to usage.json. Silently ignores write errors.
pub fn save(path: &Path, data: &UsageData) {
    if let Ok(json) = serde_json::to_string_pretty(data) {
        let _ = std::fs::write(path, json);
    }
}
