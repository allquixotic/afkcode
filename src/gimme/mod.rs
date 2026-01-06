// Copyright (c) 2025 Sean McNamara <smcnam@gmail.com>
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Gimme module: work item checkout from AGENTS.md files.
//!
//! This module provides functionality to parse AGENTS.md checklist files,
//! select work items, mark them as in-progress with unique checkout IDs,
//! and restore them on failure.

pub mod checkout;
pub mod marker;
pub mod parser;
pub mod selector;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A parsed checklist item from an AGENTS.md file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChecklistItem {
    /// Path to the AGENTS.md file containing this item.
    pub file: PathBuf,
    /// Line number (1-indexed) where the item appears.
    pub line: usize,
    /// The marker string (e.g., "[ ]", "[x]", "[ip:a3f7]").
    pub marker: String,
    /// The content/description of the checklist item.
    pub content: String,
    /// Indented sub-items or continuation lines.
    pub sub_items: Vec<String>,
    /// Unique checkout ID (set when item is checked out).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkout_id: Option<String>,
}

/// Type of checklist marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerType {
    /// `[ ]` - Incomplete task
    Incomplete,
    /// `[x]` - Completed but unverified
    Unverified,
    /// `[V]` - Verified complete
    Verified,
    /// `[ip]` or `[ip:XXXX]` - In progress
    InProgress,
    /// `[BLOCKED]` or `[BLOCKED: reason]` - Blocked
    Blocked,
    /// Unknown marker type
    Unknown,
}

impl MarkerType {
    /// Parse a marker string into a MarkerType.
    pub fn from_marker(marker: &str) -> Self {
        match marker {
            "[ ]" => MarkerType::Incomplete,
            "[x]" => MarkerType::Unverified,
            "[V]" => MarkerType::Verified,
            m if m.starts_with("[ip") => MarkerType::InProgress,
            m if m.starts_with("[BLOCKED") => MarkerType::Blocked,
            _ => MarkerType::Unknown,
        }
    }
}

/// Filters for checkout selection.
#[derive(Debug, Clone, Default)]
pub struct CheckoutFilters {
    /// Include incomplete items `[ ]`
    pub incomplete: bool,
    /// Include unverified items `[x]`
    pub unverified: bool,
    /// Include blocked items `[BLOCKED]`
    pub blocked: bool,
}

/// Request for checking out work items.
#[derive(Debug, Clone)]
pub struct CheckoutRequest {
    /// Number of items to check out.
    pub num_items: usize,
    /// Base path to search for AGENTS.md files.
    pub base_path: PathBuf,
    /// Filters for item selection.
    pub filters: CheckoutFilters,
}

/// Result of a checkout operation.
#[derive(Debug, Clone)]
pub struct CheckoutResult {
    /// Items that were checked out.
    pub items: Vec<ChecklistItem>,
    /// Files that were modified.
    pub modified_files: Vec<PathBuf>,
}

/// Generate a unique checkout ID for a subprocess.
///
/// Creates a 4-character hex ID based on timestamp and subprocess ID.
pub fn generate_checkout_id(subprocess_id: usize) -> String {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("{:04x}", (timestamp as u16) ^ (subprocess_id as u16))
}

/// Extract the checkout ID from an in-progress marker.
///
/// Returns `Some(id)` for `[ip:XXXX]` format, `None` for plain `[ip]`.
pub fn extract_checkout_id(marker: &str) -> Option<String> {
    if marker.starts_with("[ip:") && marker.ends_with(']') {
        let id = &marker[4..marker.len() - 1];
        if !id.is_empty() {
            return Some(id.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_marker_type_parsing() {
        assert_eq!(MarkerType::from_marker("[ ]"), MarkerType::Incomplete);
        assert_eq!(MarkerType::from_marker("[x]"), MarkerType::Unverified);
        assert_eq!(MarkerType::from_marker("[V]"), MarkerType::Verified);
        assert_eq!(MarkerType::from_marker("[ip]"), MarkerType::InProgress);
        assert_eq!(MarkerType::from_marker("[ip:a3f7]"), MarkerType::InProgress);
        assert_eq!(MarkerType::from_marker("[BLOCKED]"), MarkerType::Blocked);
        assert_eq!(
            MarkerType::from_marker("[BLOCKED: waiting on API]"),
            MarkerType::Blocked
        );
        assert_eq!(MarkerType::from_marker("[?]"), MarkerType::Unknown);
    }

    #[test]
    fn test_extract_checkout_id() {
        assert_eq!(extract_checkout_id("[ip:a3f7]"), Some("a3f7".to_string()));
        assert_eq!(extract_checkout_id("[ip:1234]"), Some("1234".to_string()));
        assert_eq!(extract_checkout_id("[ip]"), None);
        assert_eq!(extract_checkout_id("[ ]"), None);
    }

    #[test]
    fn test_generate_checkout_id() {
        let id1 = generate_checkout_id(0);
        let id2 = generate_checkout_id(1);
        assert_eq!(id1.len(), 4);
        assert_eq!(id2.len(), 4);
        // IDs should be different for different subprocess IDs
        // (though not guaranteed due to timing)
    }
}
