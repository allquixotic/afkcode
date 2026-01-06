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

//! Selector for randomly choosing work items with file grouping preference.

use rand::seq::SliceRandom;
use rand::thread_rng;
use std::collections::HashMap;
use std::path::PathBuf;

use super::{ChecklistItem, CheckoutFilters, MarkerType};

/// Filter items by marker type based on the given filters.
pub fn filter_items(items: Vec<ChecklistItem>, filters: &CheckoutFilters) -> Vec<ChecklistItem> {
    items
        .into_iter()
        .filter(|item| {
            let marker_type = MarkerType::from_marker(&item.marker);
            match marker_type {
                MarkerType::Incomplete => filters.incomplete,
                MarkerType::Unverified => filters.unverified,
                MarkerType::Blocked => filters.blocked,
                // Skip Verified, InProgress, Unknown
                _ => false,
            }
        })
        .collect()
}

/// Group items by their source file.
fn group_by_file(items: Vec<ChecklistItem>) -> Vec<(PathBuf, Vec<ChecklistItem>)> {
    let mut groups: HashMap<PathBuf, Vec<ChecklistItem>> = HashMap::new();
    let mut order = Vec::new();

    for item in items {
        if !groups.contains_key(&item.file) {
            order.push(item.file.clone());
        }
        groups.entry(item.file.clone()).or_default().push(item);
    }

    order
        .into_iter()
        .filter_map(|f| groups.remove(&f).map(|items| (f, items)))
        .collect()
}

/// Select n items with same-file grouping preference.
///
/// Items from the same file are preferred to keep related work together.
/// Both groups and items within groups are shuffled for randomization.
pub fn select(
    items: Vec<ChecklistItem>,
    n: usize,
    filters: &CheckoutFilters,
) -> Vec<ChecklistItem> {
    let filtered = filter_items(items, filters);

    if filtered.is_empty() {
        return vec![];
    }

    if n >= filtered.len() {
        return filtered;
    }

    let mut groups = group_by_file(filtered);
    let mut rng = thread_rng();

    // Shuffle groups and items within groups
    groups.shuffle(&mut rng);
    for (_, group_items) in &mut groups {
        group_items.shuffle(&mut rng);
    }

    // Select n items, preferring items from the same file
    let mut result = Vec::new();
    for (_, group_items) in groups {
        let take = std::cmp::min(n - result.len(), group_items.len());
        result.extend(group_items.into_iter().take(take));
        if result.len() >= n {
            break;
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_item(file: &str, marker: &str, content: &str) -> ChecklistItem {
        ChecklistItem {
            file: PathBuf::from(file),
            line: 1,
            marker: marker.to_string(),
            content: content.to_string(),
            sub_items: vec![],
            checkout_id: None,
        }
    }

    #[test]
    fn test_filter_incomplete() {
        let items = vec![
            make_item("a.md", "[ ]", "incomplete"),
            make_item("a.md", "[x]", "unverified"),
            make_item("a.md", "[V]", "verified"),
            make_item("a.md", "[ip]", "in progress"),
        ];

        let filters = CheckoutFilters {
            incomplete: true,
            unverified: false,
            blocked: false,
        };

        let result = filter_items(items, &filters);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].content, "incomplete");
    }

    #[test]
    fn test_filter_multiple() {
        let items = vec![
            make_item("a.md", "[ ]", "incomplete"),
            make_item("a.md", "[x]", "unverified"),
            make_item("a.md", "[BLOCKED]", "blocked"),
        ];

        let filters = CheckoutFilters {
            incomplete: true,
            unverified: true,
            blocked: false,
        };

        let result = filter_items(items, &filters);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_select_respects_count() {
        let items = vec![
            make_item("a.md", "[ ]", "task1"),
            make_item("a.md", "[ ]", "task2"),
            make_item("a.md", "[ ]", "task3"),
        ];

        let filters = CheckoutFilters {
            incomplete: true,
            unverified: false,
            blocked: false,
        };

        let result = select(items, 2, &filters);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_select_prefers_same_file() {
        let items = vec![
            make_item("a.md", "[ ]", "a1"),
            make_item("a.md", "[ ]", "a2"),
            make_item("b.md", "[ ]", "b1"),
            make_item("b.md", "[ ]", "b2"),
        ];

        let filters = CheckoutFilters {
            incomplete: true,
            unverified: false,
            blocked: false,
        };

        // Run multiple times to verify grouping behavior
        for _ in 0..10 {
            let result = select(items.clone(), 2, &filters);
            assert_eq!(result.len(), 2);
            // Both items should be from the same file
            assert_eq!(result[0].file, result[1].file);
        }
    }

    #[test]
    fn test_select_empty_input() {
        let items: Vec<ChecklistItem> = vec![];
        let filters = CheckoutFilters {
            incomplete: true,
            unverified: false,
            blocked: false,
        };

        let result = select(items, 2, &filters);
        assert!(result.is_empty());
    }

    #[test]
    fn test_select_more_than_available() {
        let items = vec![
            make_item("a.md", "[ ]", "task1"),
            make_item("a.md", "[ ]", "task2"),
        ];

        let filters = CheckoutFilters {
            incomplete: true,
            unverified: false,
            blocked: false,
        };

        let result = select(items, 10, &filters);
        assert_eq!(result.len(), 2);
    }
}
