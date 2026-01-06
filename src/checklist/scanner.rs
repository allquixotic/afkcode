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

//! Scanner for detecting incomplete items across all AGENTS.md files.
//!
//! This module provides deterministic completion detection by scanning
//! all AGENTS.md files in a directory tree and counting incomplete markers.

use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::gimme::{parser, ChecklistItem, MarkerType};

/// An incomplete checklist item found during scanning.
#[derive(Debug, Clone)]
pub struct IncompleteItem {
    /// Line number (1-indexed) where the item appears.
    pub line: usize,
    /// The marker string (e.g., "[ ]", "[~]", "[ip:a3f7]").
    pub marker: String,
    /// The content/description of the checklist item.
    pub content: String,
}

impl From<&ChecklistItem> for IncompleteItem {
    fn from(item: &ChecklistItem) -> Self {
        Self {
            line: item.line,
            marker: item.marker.clone(),
            content: item.content.clone(),
        }
    }
}

/// Result of scanning all AGENTS.md files for incomplete items.
#[derive(Debug, Clone)]
pub struct ScanResult {
    /// Root AGENTS.md file (directly in base_path, if found).
    /// This is typically the architecture guide, not a checklist.
    pub root_agents_md: Option<PathBuf>,

    /// All component AGENTS.md files (in subdirectories).
    pub component_checklists: Vec<PathBuf>,

    /// Total number of incomplete items across all files.
    pub total_incomplete: usize,

    /// Incomplete items grouped by file path.
    pub incomplete_by_file: HashMap<PathBuf, Vec<IncompleteItem>>,
}

impl ScanResult {
    /// Returns true if all checklists are complete (no incomplete items).
    pub fn is_complete(&self) -> bool {
        self.total_incomplete == 0
    }

    /// Returns the total number of files scanned.
    pub fn total_files(&self) -> usize {
        let root_count = if self.root_agents_md.is_some() { 1 } else { 0 };
        root_count + self.component_checklists.len()
    }

    /// Returns a summary string for logging.
    pub fn summary(&self) -> String {
        if self.is_complete() {
            format!(
                "All checklists complete ({} files scanned)",
                self.total_files()
            )
        } else {
            let files_with_incomplete = self.incomplete_by_file.len();
            format!(
                "{} incomplete items across {} files ({} files scanned)",
                self.total_incomplete,
                files_with_incomplete,
                self.total_files()
            )
        }
    }
}

/// Scan all AGENTS.md files under base_path for incomplete items.
///
/// This function:
/// 1. Discovers all AGENTS.md files recursively
/// 2. Separates the root AGENTS.md (in base_path) from component checklists (in subdirs)
/// 3. Parses each file for checklist items
/// 4. Counts items with incomplete markers: `[ ]`, `[~]`, `[ip]`, `[ip:XXXX]`
///
/// # Arguments
/// * `base_path` - The root directory to search from
///
/// # Returns
/// A `ScanResult` containing all discovered files and incomplete items.
pub fn scan_all_checklists(base_path: &Path) -> Result<ScanResult> {
    let all_files = parser::find_agents_files(base_path)?;

    // Separate root from components
    let root_agents_path = base_path.join("AGENTS.md");
    let mut root_agents_md: Option<PathBuf> = None;
    let mut component_checklists: Vec<PathBuf> = Vec::new();

    for file in &all_files {
        if file == &root_agents_path {
            root_agents_md = Some(file.clone());
        } else {
            component_checklists.push(file.clone());
        }
    }

    // Sort component checklists for deterministic ordering
    component_checklists.sort();

    // Scan all files for incomplete items
    let mut total_incomplete = 0usize;
    let mut incomplete_by_file: HashMap<PathBuf, Vec<IncompleteItem>> = HashMap::new();

    for file in &all_files {
        let items = parser::parse_file(file)?;

        let incomplete_items: Vec<IncompleteItem> = items
            .iter()
            .filter(|item| MarkerType::from_marker(&item.marker).is_incomplete())
            .map(IncompleteItem::from)
            .collect();

        if !incomplete_items.is_empty() {
            total_incomplete += incomplete_items.len();
            incomplete_by_file.insert(file.clone(), incomplete_items);
        }
    }

    Ok(ScanResult {
        root_agents_md,
        component_checklists,
        total_incomplete,
        incomplete_by_file,
    })
}

/// Quick check if any incomplete items exist without collecting details.
///
/// This is a faster alternative to `scan_all_checklists` when you only
/// need to know if work remains.
pub fn has_incomplete_items(base_path: &Path) -> Result<bool> {
    let files = parser::find_agents_files(base_path)?;

    for file in files {
        let items = parser::parse_file(&file)?;
        for item in items {
            if MarkerType::from_marker(&item.marker).is_incomplete() {
                return Ok(true);
            }
        }
    }

    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;
    use tempfile::TempDir;

    fn create_test_file(dir: &Path, relative_path: &str, content: &str) -> PathBuf {
        let path = dir.join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut file = File::create(&path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
        path
    }

    #[test]
    fn test_scan_empty_directory() {
        let dir = TempDir::new().unwrap();
        let result = scan_all_checklists(dir.path()).unwrap();

        assert!(result.root_agents_md.is_none());
        assert!(result.component_checklists.is_empty());
        assert_eq!(result.total_incomplete, 0);
        assert!(result.is_complete());
    }

    #[test]
    fn test_scan_root_only() {
        let dir = TempDir::new().unwrap();
        create_test_file(
            dir.path(),
            "AGENTS.md",
            "# Root\n- [ ] Task 1\n- [x] Task 2\n",
        );

        let result = scan_all_checklists(dir.path()).unwrap();

        assert!(result.root_agents_md.is_some());
        assert!(result.component_checklists.is_empty());
        assert_eq!(result.total_incomplete, 1);
        assert!(!result.is_complete());
    }

    #[test]
    fn test_scan_with_components() {
        let dir = TempDir::new().unwrap();
        create_test_file(dir.path(), "AGENTS.md", "# Root Architecture\n");
        create_test_file(
            dir.path(),
            "pkg/level/AGENTS.md",
            "- [ ] Level task\n- [~] Partial task\n",
        );
        create_test_file(dir.path(), "internal/game/AGENTS.md", "- [ip] In progress\n");

        let result = scan_all_checklists(dir.path()).unwrap();

        assert!(result.root_agents_md.is_some());
        assert_eq!(result.component_checklists.len(), 2);
        assert_eq!(result.total_incomplete, 3); // [ ], [~], [ip]
        assert!(!result.is_complete());
        assert_eq!(result.incomplete_by_file.len(), 2); // Two files have incomplete items
    }

    #[test]
    fn test_scan_all_complete() {
        let dir = TempDir::new().unwrap();
        create_test_file(dir.path(), "AGENTS.md", "# Root\n");
        create_test_file(dir.path(), "pkg/AGENTS.md", "- [x] Done\n- [V] Verified\n");

        let result = scan_all_checklists(dir.path()).unwrap();

        assert!(result.is_complete());
        assert_eq!(result.total_incomplete, 0);
    }

    #[test]
    fn test_scan_with_checkout_ids() {
        let dir = TempDir::new().unwrap();
        create_test_file(dir.path(), "AGENTS.md", "- [ip:a3f7] Checked out task\n");

        let result = scan_all_checklists(dir.path()).unwrap();

        assert_eq!(result.total_incomplete, 1);
        let items = result.incomplete_by_file.values().next().unwrap();
        assert_eq!(items[0].marker, "[ip:a3f7]");
    }

    #[test]
    fn test_has_incomplete_items() {
        let dir = TempDir::new().unwrap();
        create_test_file(dir.path(), "AGENTS.md", "- [x] Complete\n");

        assert!(!has_incomplete_items(dir.path()).unwrap());

        create_test_file(dir.path(), "sub/AGENTS.md", "- [ ] Todo\n");

        assert!(has_incomplete_items(dir.path()).unwrap());
    }

    #[test]
    fn test_summary() {
        let dir = TempDir::new().unwrap();
        create_test_file(dir.path(), "AGENTS.md", "# Root\n- [ ] Task\n");
        create_test_file(dir.path(), "sub/AGENTS.md", "- [ ] Another\n- [~] Partial\n");

        let result = scan_all_checklists(dir.path()).unwrap();

        let summary = result.summary();
        assert!(summary.contains("3 incomplete items"));
        assert!(summary.contains("2 files"));
    }
}
