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

//! Verifier module: LLM-based verification of implementation completeness.
//!
//! After workers report completion (scanner shows zero incomplete items),
//! the verifier can optionally run to audit "complete" items and find
//! missing features by comparing implementations to original sources.

use anyhow::Result;
use std::fs;
use std::path::PathBuf;

use crate::checklist::scanner::scan_all_checklists;
use crate::llm::LlmToolChain;
use crate::logger::Logger;
use crate::prompts::DEFAULT_VERIFIER_PROMPT;
use crate::runner::{log_message, stream_outputs};

/// Configuration for the verifier phase.
#[derive(Debug, Clone)]
pub struct VerifierConfig {
    /// Path to custom verifier prompt file (uses default if None)
    pub prompt_path: Option<PathBuf>,
    /// Base path for scanning AGENTS.md files
    pub checklist_dir: PathBuf,
    /// Completion token (for placeholder substitution)
    pub completion_token: String,
}

/// Result of a verification run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifierResult {
    /// Verifier found new work items (count of new incomplete items)
    FoundWork(usize),
    /// Verifier found no new work - project is truly complete
    NoNewWork,
}

/// Run the verifier LLM to audit implementation completeness.
///
/// The verifier:
/// 1. Scans all AGENTS.md files to count current incomplete items
/// 2. Runs an LLM with the verification prompt
/// 3. The LLM reads code and audits "complete" items, adding new work if found
/// 4. Scans again to detect if new incomplete items were added
///
/// Returns `VerifierResult::FoundWork(n)` if n new items were added,
/// or `VerifierResult::NoNewWork` if no new work was found.
pub fn run_verifier(
    config: &VerifierConfig,
    tool_chain: &mut LlmToolChain,
    logger: &mut Option<Logger>,
) -> Result<VerifierResult> {
    log_message(logger, "Starting verification phase...");

    // Count incomplete items before verification
    let before_scan = scan_all_checklists(&config.checklist_dir)?;
    let before_count = before_scan.total_incomplete;
    log_message(
        logger,
        &format!(
            "Before verification: {} incomplete items across {} files",
            before_count,
            before_scan.total_files()
        ),
    );

    // Build the verifier prompt
    let prompt = build_verifier_prompt(config)?;

    // Run the verifier LLM
    log_message(logger, "Running verifier LLM...");
    let (stdout, stderr) = tool_chain.invoke_with_fallback(&prompt, logger)?;
    stream_outputs("verifier", &stdout, &stderr, logger);

    // Count incomplete items after verification
    let after_scan = scan_all_checklists(&config.checklist_dir)?;
    let after_count = after_scan.total_incomplete;
    log_message(
        logger,
        &format!(
            "After verification: {} incomplete items across {} files",
            after_count,
            after_scan.total_files()
        ),
    );

    // Determine result based on change in incomplete count
    if after_count > before_count {
        let new_items = after_count - before_count;
        log_message(
            logger,
            &format!("Verifier found {} new work items", new_items),
        );
        Ok(VerifierResult::FoundWork(new_items))
    } else if after_count > 0 {
        // Items exist but weren't newly added (possibly were already there)
        log_message(
            logger,
            &format!(
                "Verifier did not add new items, but {} incomplete items remain",
                after_count
            ),
        );
        Ok(VerifierResult::NoNewWork)
    } else {
        log_message(logger, "Verifier confirmed: all work is complete");
        Ok(VerifierResult::NoNewWork)
    }
}

/// Build the verifier prompt from config.
fn build_verifier_prompt(config: &VerifierConfig) -> Result<String> {
    let template = if let Some(ref path) = config.prompt_path {
        fs::read_to_string(path)?
    } else {
        DEFAULT_VERIFIER_PROMPT.to_string()
    };

    // Get the scan result to populate file lists
    let scan = scan_all_checklists(&config.checklist_dir)?;

    // Build root AGENTS.md reference
    let root_ref = if let Some(ref root) = scan.root_agents_md {
        format!("@{}", root.display())
    } else {
        "No root AGENTS.md found".to_string()
    };

    // Build component checklists list
    let component_refs: Vec<String> = scan
        .component_checklists
        .iter()
        .map(|p| format!("@{}", p.display()))
        .collect();
    let components_list = if component_refs.is_empty() {
        "No component AGENTS.md files found".to_string()
    } else {
        component_refs.join("\n")
    };

    // Substitute placeholders
    let prompt = template
        .replace("{root_agents_md}", &root_ref)
        .replace("{component_checklists}", &components_list)
        .replace("{completion_token}", &config.completion_token);

    Ok(prompt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::TempDir;

    fn create_test_file(dir: &std::path::Path, relative_path: &str, content: &str) -> PathBuf {
        let path = dir.join(relative_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let mut file = File::create(&path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
        path
    }

    #[test]
    fn test_build_verifier_prompt_with_files() {
        let dir = TempDir::new().unwrap();
        create_test_file(dir.path(), "AGENTS.md", "# Root\n- [x] Done\n");
        create_test_file(dir.path(), "pkg/AGENTS.md", "- [x] Component done\n");

        let config = VerifierConfig {
            prompt_path: None,
            checklist_dir: dir.path().to_path_buf(),
            completion_token: "__ALL_TASKS_COMPLETE__".to_string(),
        };

        let prompt = build_verifier_prompt(&config).unwrap();

        // Should contain root reference
        assert!(prompt.contains("AGENTS.md"));
        // Should not contain the completion token placeholder
        assert!(!prompt.contains("{completion_token}"));
        // Should contain substituted token
        assert!(prompt.contains("__ALL_TASKS_COMPLETE__"));
    }

    #[test]
    fn test_build_verifier_prompt_custom_file() {
        let dir = TempDir::new().unwrap();
        create_test_file(dir.path(), "AGENTS.md", "# Root\n");

        let custom_prompt = "Custom verifier: {root_agents_md}\nToken: {completion_token}";
        let prompt_path = create_test_file(dir.path(), "custom_prompt.md", custom_prompt);

        let config = VerifierConfig {
            prompt_path: Some(prompt_path),
            checklist_dir: dir.path().to_path_buf(),
            completion_token: "DONE".to_string(),
        };

        let prompt = build_verifier_prompt(&config).unwrap();

        assert!(prompt.contains("Custom verifier:"));
        assert!(prompt.contains("Token: DONE"));
    }
}
