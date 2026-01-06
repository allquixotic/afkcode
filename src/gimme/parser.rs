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

//! Parser for AGENTS.md checklist files.

use anyhow::Result;
use once_cell::sync::Lazy;
use regex::Regex;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use super::ChecklistItem;

/// Pattern matching checklist items.
/// Matches: `- [ ] task`, `- [~] task`, `- [x] task`, `- [V] task`, `- [ip] task`, `- [ip:xxxx] task`, `- [BLOCKED] task`
static CHECKLIST_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"^(\s*)-\s*\[([ ~xV]|ip(?::[a-f0-9]+)?|BLOCKED(?::[^\]]*)?)\]\s*(.*)$"#).unwrap()
});

/// Pattern matching sub-items (indented lines starting with dash).
static SUB_ITEM_PATTERN: Lazy<Regex> = Lazy::new(|| Regex::new(r#"^(\s+)-\s+(.*)$"#).unwrap());

/// Find all AGENTS.md files under the given base path.
pub fn find_agents_files(base_path: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    for entry in WalkDir::new(base_path).follow_links(true) {
        let entry = entry?;
        if entry.file_type().is_file() && entry.file_name() == "AGENTS.md" {
            files.push(entry.path().to_path_buf());
        }
    }

    Ok(files)
}

/// Parse a single AGENTS.md file and return all checklist items.
pub fn parse_file(path: &Path) -> Result<Vec<ChecklistItem>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let mut items = Vec::new();
    let mut current_item: Option<ChecklistItem> = None;
    let mut current_indent = 0usize;

    for (line_idx, line_result) in reader.lines().enumerate() {
        let line = line_result?;
        let line_num = line_idx + 1; // 1-indexed

        // Check if this is a checklist item
        if let Some(caps) = CHECKLIST_PATTERN.captures(&line) {
            // Save any previous item
            if let Some(item) = current_item.take() {
                items.push(item);
            }

            let indent = caps.get(1).map_or(0, |m| m.as_str().len());
            let marker_content = caps.get(2).map_or("", |m| m.as_str());
            let marker = format!("[{}]", marker_content);
            let content = caps.get(3).map_or("", |m| m.as_str()).to_string();

            current_item = Some(ChecklistItem {
                file: path.to_path_buf(),
                line: line_num,
                marker,
                content,
                sub_items: Vec::new(),
                checkout_id: None,
            });
            current_indent = indent;
            continue;
        }

        // Check if this is a sub-item for the current checklist item
        if let Some(ref mut item) = current_item {
            if let Some(caps) = SUB_ITEM_PATTERN.captures(&line) {
                let sub_indent = caps.get(1).map_or(0, |m| m.as_str().len());
                // Sub-item should be more indented than the parent
                if sub_indent > current_indent {
                    item.sub_items.push(line.clone());
                    continue;
                }
            }

            // Check for continuation lines (indented content that's not a new checklist item)
            let trimmed = line.trim();
            let min_indent = current_indent + 2;
            let leading_spaces = line.len() - line.trim_start().len();

            if !trimmed.is_empty() && leading_spaces >= min_indent {
                // This is continuation content (code blocks, etc.)
                item.sub_items.push(line.clone());
                continue;
            }

            // Empty lines within sub-items section (preserve if inside code block)
            if trimmed.is_empty() && !item.sub_items.is_empty() {
                // Check if we're inside a code block
                if count_backticks(&item.sub_items) % 2 == 1 {
                    item.sub_items.push(line.clone());
                    continue;
                }
            }

            // If we hit a non-indented non-empty line, end the current item
            if !trimmed.is_empty() && !line.starts_with(' ') && !line.starts_with('\t') {
                items.push(item.clone());
                current_item = None;
            }
        }
    }

    // Don't forget the last item
    if let Some(item) = current_item {
        items.push(item);
    }

    Ok(items)
}

/// Count how many lines contain triple backticks.
fn count_backticks(lines: &[String]) -> usize {
    lines.iter().filter(|line| line.contains("```")).count()
}

/// Parse all AGENTS.md files under base_path and return all checklist items.
pub fn parse_all(base_path: &Path) -> Result<Vec<ChecklistItem>> {
    let files = find_agents_files(base_path)?;
    let mut all_items = Vec::new();

    for file in files {
        let items = parse_file(&file)?;
        all_items.extend(items);
    }

    Ok(all_items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn create_test_file(dir: &Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let mut file = File::create(&path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
        path
    }

    #[test]
    fn test_parse_simple_checklist() {
        let dir = TempDir::new().unwrap();
        let content = r#"# Tasks

- [ ] First task
- [x] Completed task
- [V] Verified task
- [ip] In progress task
- [BLOCKED] Blocked task
"#;
        let path = create_test_file(dir.path(), "AGENTS.md", content);
        let items = parse_file(&path).unwrap();

        assert_eq!(items.len(), 5);
        assert_eq!(items[0].marker, "[ ]");
        assert_eq!(items[0].content, "First task");
        assert_eq!(items[1].marker, "[x]");
        assert_eq!(items[2].marker, "[V]");
        assert_eq!(items[3].marker, "[ip]");
        assert_eq!(items[4].marker, "[BLOCKED]");
    }

    #[test]
    fn test_parse_with_checkout_id() {
        let dir = TempDir::new().unwrap();
        let content = "- [ip:a3f7] Task with checkout ID\n";
        let path = create_test_file(dir.path(), "AGENTS.md", content);
        let items = parse_file(&path).unwrap();

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].marker, "[ip:a3f7]");
        assert_eq!(items[0].content, "Task with checkout ID");
    }

    #[test]
    fn test_parse_with_sub_items() {
        let dir = TempDir::new().unwrap();
        let content = r#"- [ ] Main task
  - Sub-item one
  - Sub-item two
- [ ] Another task
"#;
        let path = create_test_file(dir.path(), "AGENTS.md", content);
        let items = parse_file(&path).unwrap();

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].content, "Main task");
        assert_eq!(items[0].sub_items.len(), 2);
        assert!(items[0].sub_items[0].contains("Sub-item one"));
    }

    #[test]
    fn test_find_agents_files() {
        let dir = TempDir::new().unwrap();
        create_test_file(dir.path(), "AGENTS.md", "- [ ] Root task\n");
        create_test_file(dir.path(), "subdir/AGENTS.md", "- [ ] Sub task\n");
        create_test_file(dir.path(), "other.md", "Not an agents file\n");

        let files = find_agents_files(dir.path()).unwrap();
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn test_parse_all() {
        let dir = TempDir::new().unwrap();
        create_test_file(dir.path(), "AGENTS.md", "- [ ] Root task\n");
        create_test_file(dir.path(), "subdir/AGENTS.md", "- [ ] Sub task\n");

        let items = parse_all(dir.path()).unwrap();
        assert_eq!(items.len(), 2);
    }
}
