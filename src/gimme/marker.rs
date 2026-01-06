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

//! Atomic file marking for checklist items.

use anyhow::{Context, Result};
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

use super::{generate_checkout_id, ChecklistItem};

/// Pattern to match and replace checklist markers.
static MARKER_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"^(\s*-\s*)\[([ xV]|ip(?::[a-f0-9]+)?|BLOCKED(?::[^\]]*)?)\](.*)$"#).unwrap()
});

/// Mark items as in-progress with unique checkout IDs.
///
/// Updates the items in-place with their checkout IDs, then atomically
/// updates the source files.
pub fn mark_in_progress(items: &mut [ChecklistItem], subprocess_id: usize) -> Result<Vec<PathBuf>> {
    // Assign checkout IDs to items
    for item in items.iter_mut() {
        item.checkout_id = Some(generate_checkout_id(subprocess_id));
    }

    // Group items by file
    let mut by_file: HashMap<PathBuf, Vec<&ChecklistItem>> = HashMap::new();
    for item in items.iter() {
        by_file.entry(item.file.clone()).or_default().push(item);
    }

    let mut modified_files = Vec::new();

    for (path, file_items) in by_file {
        let lines_to_update: HashMap<usize, &ChecklistItem> =
            file_items.iter().map(|i| (i.line, *i)).collect();

        // Read file
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let lines: Vec<&str> = content.lines().collect();

        // Build new content
        let mut new_lines = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            let line_num = i + 1; // 1-indexed

            if let Some(item) = lines_to_update.get(&line_num) {
                if let Some(checkout_id) = &item.checkout_id {
                    new_lines.push(replace_marker_with_ip(line, checkout_id));
                } else {
                    new_lines.push(line.to_string());
                }
            } else {
                new_lines.push(line.to_string());
            }
        }

        // Atomic write: temp file + rename
        atomic_write(&path, &new_lines.join("\n"))?;
        modified_files.push(path);
    }

    Ok(modified_files)
}

/// Replace the marker in a line with [ip:ID].
fn replace_marker_with_ip(line: &str, checkout_id: &str) -> String {
    if let Some(caps) = MARKER_PATTERN.captures(line) {
        format!(
            "{}[ip:{}]{}",
            caps.get(1).map_or("", |m| m.as_str()),
            checkout_id,
            caps.get(3).map_or("", |m| m.as_str())
        )
    } else {
        line.to_string()
    }
}

/// Restore an item back to incomplete `[ ]` status.
///
/// Searches for the item's unique checkout ID and replaces it.
/// Returns `true` if the item was found and restored, `false` if not found.
pub fn restore_item(item: &ChecklistItem) -> Result<bool> {
    let checkout_id = match &item.checkout_id {
        Some(id) => id,
        None => {
            eprintln!(
                "Warning: Cannot restore item without checkout ID: {}",
                item.content
            );
            return Ok(false);
        }
    };

    let pattern = format!("[ip:{}]", checkout_id);

    // Read the file
    let content = fs::read_to_string(&item.file)
        .with_context(|| format!("Failed to read {}", item.file.display()))?;

    // Check if the pattern exists
    if !content.contains(&pattern) {
        eprintln!(
            "Warning: Checkout ID {} not found in {}, item may have been completed",
            checkout_id,
            item.file.display()
        );
        return Ok(false);
    }

    // Replace [ip:XXXX] with [ ]
    let new_content = content.replace(&pattern, "[ ]");

    // Atomic write
    atomic_write(&item.file, &new_content)?;

    Ok(true)
}

/// Validate that items still exist at their expected positions.
///
/// This is a safety check before marking to ensure the file hasn't been
/// modified in unexpected ways.
pub fn validate_items(items: &[ChecklistItem]) -> Result<()> {
    // Group by file
    let mut by_file: HashMap<PathBuf, Vec<&ChecklistItem>> = HashMap::new();
    for item in items {
        by_file.entry(item.file.clone()).or_default().push(item);
    }

    for (path, file_items) in by_file {
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let lines: Vec<&str> = content.lines().collect();

        for item in file_items {
            let line_idx = item.line - 1;
            if line_idx >= lines.len() {
                anyhow::bail!(
                    "Item at line {} no longer exists in {} (file has {} lines)",
                    item.line,
                    path.display(),
                    lines.len()
                );
            }

            let line = lines[line_idx];
            // Verify the marker matches
            if !line.contains(&item.marker) {
                anyhow::bail!(
                    "Item at line {} in {} has changed: expected marker {}, got: {}",
                    item.line,
                    path.display(),
                    item.marker,
                    line
                );
            }
        }
    }

    Ok(())
}

/// Atomically write content to a file using temp file + rename.
fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));

    // Create temp file in the same directory
    let mut temp_file = NamedTempFile::new_in(dir)
        .with_context(|| format!("Failed to create temp file in {}", dir.display()))?;

    // Write content
    temp_file
        .write_all(content.as_bytes())
        .with_context(|| "Failed to write to temp file")?;

    // Add trailing newline if content doesn't end with one
    if !content.ends_with('\n') {
        temp_file.write_all(b"\n")?;
    }

    // Preserve original permissions if the file exists
    if path.exists() {
        if let Ok(metadata) = fs::metadata(path) {
            let _ = fs::set_permissions(temp_file.path(), metadata.permissions());
        }
    }

    // Atomic rename
    temp_file
        .persist(path)
        .with_context(|| format!("Failed to persist temp file to {}", path.display()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write as IoWrite;
    use tempfile::TempDir;

    fn create_test_file(dir: &Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(name);
        let mut file = File::create(&path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
        path
    }

    #[test]
    fn test_replace_marker_with_ip() {
        let line = "- [ ] Some task";
        let result = replace_marker_with_ip(line, "a3f7");
        assert_eq!(result, "- [ip:a3f7] Some task");
    }

    #[test]
    fn test_replace_marker_indented() {
        let line = "  - [ ] Indented task";
        let result = replace_marker_with_ip(line, "1234");
        assert_eq!(result, "  - [ip:1234] Indented task");
    }

    #[test]
    fn test_mark_in_progress() {
        let dir = TempDir::new().unwrap();
        let content = "- [ ] Task one\n- [ ] Task two\n";
        let path = create_test_file(dir.path(), "AGENTS.md", content);

        let mut items = vec![ChecklistItem {
            file: path.clone(),
            line: 1,
            marker: "[ ]".to_string(),
            content: "Task one".to_string(),
            sub_items: vec![],
            checkout_id: None,
        }];

        let modified = mark_in_progress(&mut items, 0).unwrap();
        assert_eq!(modified.len(), 1);

        // Verify the file was updated
        let new_content = fs::read_to_string(&path).unwrap();
        assert!(new_content.contains("[ip:"));
        assert!(new_content.contains("Task one"));
        // Second task should be unchanged
        assert!(new_content.contains("- [ ] Task two"));
    }

    #[test]
    fn test_restore_item() {
        let dir = TempDir::new().unwrap();
        let content = "- [ip:a3f7] Task one\n";
        let path = create_test_file(dir.path(), "AGENTS.md", content);

        let item = ChecklistItem {
            file: path.clone(),
            line: 1,
            marker: "[ip:a3f7]".to_string(),
            content: "Task one".to_string(),
            sub_items: vec![],
            checkout_id: Some("a3f7".to_string()),
        };

        let restored = restore_item(&item).unwrap();
        assert!(restored);

        let new_content = fs::read_to_string(&path).unwrap();
        assert!(new_content.contains("- [ ] Task one"));
        assert!(!new_content.contains("[ip:"));
    }

    #[test]
    fn test_restore_item_not_found() {
        let dir = TempDir::new().unwrap();
        let content = "- [ ] Task one\n";
        let path = create_test_file(dir.path(), "AGENTS.md", content);

        let item = ChecklistItem {
            file: path.clone(),
            line: 1,
            marker: "[ip:a3f7]".to_string(),
            content: "Task one".to_string(),
            sub_items: vec![],
            checkout_id: Some("a3f7".to_string()),
        };

        let restored = restore_item(&item).unwrap();
        assert!(!restored);
    }

    #[test]
    fn test_validate_items_success() {
        let dir = TempDir::new().unwrap();
        let content = "- [ ] Task one\n";
        let path = create_test_file(dir.path(), "AGENTS.md", content);

        let items = vec![ChecklistItem {
            file: path.clone(),
            line: 1,
            marker: "[ ]".to_string(),
            content: "Task one".to_string(),
            sub_items: vec![],
            checkout_id: None,
        }];

        assert!(validate_items(&items).is_ok());
    }

    #[test]
    fn test_validate_items_marker_changed() {
        let dir = TempDir::new().unwrap();
        let content = "- [x] Task one\n";
        let path = create_test_file(dir.path(), "AGENTS.md", content);

        let items = vec![ChecklistItem {
            file: path.clone(),
            line: 1,
            marker: "[ ]".to_string(), // Expected [ ] but file has [x]
            content: "Task one".to_string(),
            sub_items: vec![],
            checkout_id: None,
        }];

        assert!(validate_items(&items).is_err());
    }
}
