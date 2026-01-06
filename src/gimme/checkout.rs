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

//! File-lock based checkout for work items.

use anyhow::{Context, Result};
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::marker;
use super::parser;
use super::selector;
use super::{ChecklistItem, CheckoutRequest, CheckoutResult};

const LOCK_FILE_NAME: &str = ".gimme.lock";

/// File-based lock for atomic checkout operations.
pub struct FileLock {
    #[allow(dead_code)]
    lock_file: File,
    #[allow(dead_code)]
    lock_path: PathBuf,
}

impl FileLock {
    /// Acquire an exclusive lock with timeout.
    ///
    /// Creates a lock file in the base path and acquires an exclusive flock.
    /// Blocks until the lock is acquired or timeout is reached.
    pub fn acquire(base_path: &Path, timeout: Duration) -> Result<Self> {
        let lock_path = base_path.join(LOCK_FILE_NAME);

        let lock_file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&lock_path)
            .with_context(|| format!("Failed to open lock file: {}", lock_path.display()))?;

        let start = Instant::now();
        loop {
            match lock_file.try_lock_exclusive() {
                Ok(()) => {
                    return Ok(Self {
                        lock_file,
                        lock_path,
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if start.elapsed() >= timeout {
                        anyhow::bail!(
                            "Timeout acquiring gimme lock after {:?}",
                            start.elapsed()
                        );
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(e) => {
                    return Err(e)
                        .with_context(|| format!("Failed to lock: {}", lock_path.display()));
                }
            }
        }
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        // Release the lock
        let _ = self.lock_file.unlock();
    }
}

/// Checkout work items atomically.
///
/// Acquires an exclusive lock, parses AGENTS.md files, selects items
/// based on filters, marks them as in-progress, and returns the result.
pub fn checkout(request: CheckoutRequest, subprocess_id: usize) -> Result<CheckoutResult> {
    // Acquire exclusive lock
    let _lock = FileLock::acquire(&request.base_path, Duration::from_secs(30))
        .with_context(|| "Failed to acquire gimme lock")?;

    // Parse all AGENTS.md files
    let items = parser::parse_all(&request.base_path)
        .with_context(|| format!("Failed to parse AGENTS.md files in {}", request.base_path.display()))?;

    if items.is_empty() {
        return Ok(CheckoutResult {
            items: vec![],
            modified_files: vec![],
        });
    }

    // Select items based on filters
    let mut selected = selector::select(items, request.num_items, &request.filters);

    if selected.is_empty() {
        return Ok(CheckoutResult {
            items: vec![],
            modified_files: vec![],
        });
    }

    // Validate items before marking
    marker::validate_items(&selected).with_context(|| "Item validation failed")?;

    // Mark items as in progress with checkout IDs
    let modified_files = marker::mark_in_progress(&mut selected, subprocess_id)
        .with_context(|| "Failed to mark items as in-progress")?;

    Ok(CheckoutResult {
        items: selected,
        modified_files,
    })
    // Lock is released here when _lock is dropped
}

/// Build a prompt section describing the checked-out work items.
///
/// This is injected into the worker prompt so the LLM knows what to work on.
pub fn build_work_items_prompt(items: &[ChecklistItem]) -> String {
    if items.is_empty() {
        return String::new();
    }

    let mut prompt = String::from("You have been assigned the following work items:\n\n");

    for item in items {
        prompt.push_str(&format!(
            "- {} (from {}:{})\n",
            item.content,
            item.file.display(),
            item.line
        ));

        for sub in &item.sub_items {
            prompt.push_str(&format!("  {}\n", sub));
        }
    }

    prompt.push_str("\nFocus on completing these assigned items. ");
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gimme::CheckoutFilters;
    use std::fs;
    use std::io::Write;
    use tempfile::TempDir;

    fn create_test_file(dir: &Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut file = File::create(&path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
        path
    }

    #[test]
    fn test_file_lock() {
        let dir = TempDir::new().unwrap();

        // First lock should succeed
        let lock1 = FileLock::acquire(dir.path(), Duration::from_millis(100));
        assert!(lock1.is_ok());

        // Second lock should fail (timeout)
        let lock2 = FileLock::acquire(dir.path(), Duration::from_millis(100));
        assert!(lock2.is_err());

        // Drop first lock
        drop(lock1);

        // Now second lock should succeed
        let lock3 = FileLock::acquire(dir.path(), Duration::from_millis(100));
        assert!(lock3.is_ok());
    }

    #[test]
    fn test_checkout() {
        let dir = TempDir::new().unwrap();
        create_test_file(
            dir.path(),
            "AGENTS.md",
            "- [ ] Task one\n- [ ] Task two\n- [x] Done task\n",
        );

        let request = CheckoutRequest {
            num_items: 1,
            base_path: dir.path().to_path_buf(),
            filters: CheckoutFilters {
                incomplete: true,
                unverified: false,
                blocked: false,
            },
        };

        let result = checkout(request, 0).unwrap();
        assert_eq!(result.items.len(), 1);
        assert!(result.items[0].checkout_id.is_some());

        // Verify file was updated
        let content = fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
        assert!(content.contains("[ip:"));
    }

    #[test]
    fn test_checkout_no_matching_items() {
        let dir = TempDir::new().unwrap();
        create_test_file(dir.path(), "AGENTS.md", "- [x] All done\n- [V] Verified\n");

        let request = CheckoutRequest {
            num_items: 1,
            base_path: dir.path().to_path_buf(),
            filters: CheckoutFilters {
                incomplete: true,
                unverified: false,
                blocked: false,
            },
        };

        let result = checkout(request, 0).unwrap();
        assert!(result.items.is_empty());
    }

    #[test]
    fn test_build_work_items_prompt() {
        let items = vec![
            ChecklistItem {
                file: PathBuf::from("/path/to/AGENTS.md"),
                line: 5,
                marker: "[ip:a3f7]".to_string(),
                content: "Implement feature X".to_string(),
                sub_items: vec!["  - Add tests".to_string()],
                checkout_id: Some("a3f7".to_string()),
            },
        ];

        let prompt = build_work_items_prompt(&items);
        assert!(prompt.contains("Implement feature X"));
        assert!(prompt.contains("/path/to/AGENTS.md:5"));
        assert!(prompt.contains("Add tests"));
    }
}
