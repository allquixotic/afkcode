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

use anyhow::{Context, Result};
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use crate::cli::RunMode;
use crate::constants::{render_core_standing_orders, DEFAULT_COMPLETION_TOKEN};
use crate::llm::{LlmToolChain, ModelConfig};
use crate::logger::Logger;
use crate::parallel::{self, ParallelConfig};
use crate::runner::{run_controller_worker_loop, run_worker_loop, RunConfig};
use crate::wakelock::WakeLock;

pub fn cmd_run(
    checklist: PathBuf,
    controller_prompt: String,
    worker_prompt: String,
    completion_token: String,
    sleep_seconds: u64,
    mode: RunMode,
    skip_audit: bool,
    audit_orders_path: Option<PathBuf>,
    commit_audit: bool,
    tools: String,
    log_file: String,
    model_config: ModelConfig,
    shutdown_flag: Arc<AtomicBool>,
    num_instances: usize,
    warmup_delay: u64,
    gimme_enabled: bool,
    gimme_base_path: PathBuf,
    items_per_instance: usize,
) -> Result<()> {
    // Acquire wake lock to prevent system sleep during LLM execution.
    // Uses OS-native facilities that are automatically released when the process exits,
    // even if forcibly killed (SIGKILL), so the system can still sleep afterward.
    let _wake_guard = match WakeLock::try_acquire() {
        Some(guard) => {
            println!("System sleep inhibited while LLM subprocesses are running");
            Some(guard)
        }
        None => {
            eprintln!("Warning: Could not acquire wake lock. System may sleep during execution.");
            None
        }
    };

    // Ensure checklist file exists
    if let Some(parent) = checklist.parent() {
        fs::create_dir_all(parent)?;
    }
    if !checklist.exists() {
        fs::File::create(&checklist)?;
    }

    // Initialize logger
    let mut logger = match Logger::new(&log_file) {
        Ok(log) => {
            println!("Logging to: {}", log_file);
            Some(log)
        }
        Err(e) => {
            eprintln!("Warning: Failed to create log file '{}': {}", log_file, e);
            eprintln!("Continuing without logging to file.");
            None
        }
    };

    if let Some(log) = logger.as_mut() {
        println!("Logging to: {}", log_file);
        let _ = log.logln(&format!("Logging to: {}", log_file));
    }

    let checklist_path_str = checklist
        .to_str()
        .context("Checklist path contains invalid UTF-8")?
        .to_string();

    let run_config = RunConfig {
        checklist: checklist.clone(),
        checklist_path_str,
        controller_prompt,
        worker_prompt,
        completion_token,
        sleep_seconds,
        mode,
        skip_audit,
        audit_orders_path,
        commit_audit,
        shutdown_flag,
    };

    // Use parallel runner if num_instances > 1
    if num_instances > 1 {
        let parallel_config = ParallelConfig {
            num_instances,
            warmup_delay: Duration::from_secs(warmup_delay),
            gimme_enabled,
            gimme_base_path,
            items_per_instance,
            run_config,
            model_config,
            tools,
            log_file,
        };
        return parallel::run_parallel(parallel_config);
    }

    // Single instance mode (original behavior)
    let mut tool_chain = LlmToolChain::with_models(&tools, &model_config)?;

    match run_config.mode {
        RunMode::Worker => run_worker_loop(&run_config, &mut tool_chain, &mut logger)?,
        RunMode::Controller => {
            run_controller_worker_loop(&run_config, &mut tool_chain, &mut logger)?
        }
    }

    Ok(())
}

pub fn cmd_init(checklist: PathBuf, title: Option<String>, examples: bool) -> Result<()> {
    if checklist.exists() {
        anyhow::bail!("Checklist file already exists: {}", checklist.display());
    }

    if let Some(parent) = checklist.parent() {
        fs::create_dir_all(parent)?;
    }

    let title_text = title.unwrap_or_else(|| "Project Checklist".to_string());

    let mut content = format!("# {}\n\n", title_text);
    let core_orders = render_core_standing_orders(DEFAULT_COMPLETION_TOKEN);
    content.push_str(&core_orders);
    content.push_str("\n\n# Project Standing Orders\n\n");

    if examples {
        content.push_str(
            r#"# High-Level Requirements
- [ ] Requirement 1
- [ ] Requirement 2
- [ ] Requirement 3

# Tasks
- [ ] Task 1
- [ ] Task 2
    - [ ] Subtask 2.1
    - [ ] Subtask 2.2

# Notes
Additional context or documentation goes here.
"#,
        );
    }

    fs::write(&checklist, content)?;
    println!("Created checklist: {}", checklist.display());

    Ok(())
}

pub fn cmd_generate(checklist: PathBuf, prompt: String, tools: String, model_config: ModelConfig) -> Result<()> {
    if checklist.exists() {
        anyhow::bail!("Checklist file already exists: {}", checklist.display());
    }

    let generation_prompt = format!(
        r#"Generate a detailed project checklist in Markdown format based on this description:

{}

The checklist should:
1. Start with a clear project title as an H1 heading
2. Include high-level requirements
3. Break down requirements into specific tasks
4. Use [ ] for unchecked items
5. Include sub-items where appropriate (indented with 4 spaces)
6. Be actionable and specific
7. Focus on design and coding activities only
8. Do NOT include the standing orders (they will be prepended automatically)

Output ONLY the checklist content in Markdown format, nothing else.
"#,
        prompt
    );

    println!("Generating checklist...");
    let mut tool_chain = LlmToolChain::with_models(&tools, &model_config)?;
    let mut logger = None;
    let (stdout, _stderr) = tool_chain.invoke_with_fallback(&generation_prompt, &mut logger)?;

    if stdout.trim().is_empty() {
        anyhow::bail!("LLM returned empty response");
    }

    // Combine standing orders with generated content
    let mut full_content = String::new();

    // Extract title if present in generated content
    let lines: Vec<&str> = stdout.lines().collect();
    let mut content_start = 0;

    if let Some(first_line) = lines.first() {
        if first_line.starts_with("# ") {
            full_content.push_str(first_line);
            full_content.push_str("\n\n");
            content_start = 1;
        } else {
            full_content.push_str("# Generated Project Checklist\n\n");
        }
    }

    let core_orders = render_core_standing_orders(DEFAULT_COMPLETION_TOKEN);
    full_content.push_str(&core_orders);
    full_content.push_str("\n\n# Project Standing Orders\n\n");

    // Add the rest of the generated content
    for line in &lines[content_start..] {
        full_content.push_str(line);
        full_content.push('\n');
    }

    if let Some(parent) = checklist.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(&checklist, full_content)?;
    println!("Generated checklist: {}", checklist.display());

    Ok(())
}

pub fn cmd_add(checklist: PathBuf, item: String, sub: bool, section: Option<String>) -> Result<()> {
    if !checklist.exists() {
        anyhow::bail!("Checklist file does not exist: {}", checklist.display());
    }

    let content = fs::read_to_string(&checklist)?;
    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();

    let indent = if sub { "    " } else { "" };
    let new_item = format!("{}- [ ] {}", indent, item);

    let insert_pos = if let Some(section_name) = section {
        // Find the section and insert after it
        let mut found = false;
        let mut pos = lines.len();

        for (i, line) in lines.iter().enumerate() {
            if line.starts_with('#') && line.contains(&section_name) {
                found = true;
                // Skip to the next non-empty line after the section header
                pos = i + 1;
                while pos < lines.len() && lines[pos].trim().is_empty() {
                    pos += 1;
                }
                break;
            }
        }

        if !found {
            anyhow::bail!("Section '{}' not found in checklist", section_name);
        }
        pos
    } else {
        // Append to end
        lines.len()
    };

    lines.insert(insert_pos, new_item);

    let updated_content = lines.join("\n") + "\n";
    fs::write(&checklist, updated_content)?;

    println!("Added item to {}", checklist.display());

    Ok(())
}

pub fn cmd_add_batch(checklist: PathBuf, description: String, tools: String, model_config: ModelConfig) -> Result<()> {
    if !checklist.exists() {
        anyhow::bail!("Checklist file does not exist: {}", checklist.display());
    }

    let checklist_path = checklist
        .to_str()
        .context("Checklist path contains invalid UTF-8")?;

    let batch_prompt = format!(
        r#"@{}

Review the checklist above. Based on this high-level description:

{}

Generate a list of specific, actionable checklist items to add. Output ONLY the checklist items in this exact format:
- [ ] Item 1
- [ ] Item 2
    - [ ] Sub-item 2.1

Do not include explanations, headers, or any other text. Just the checkbox items.
"#,
        checklist_path, description
    );

    println!("Generating items...");
    let mut tool_chain = LlmToolChain::with_models(&tools, &model_config)?;
    let mut logger = None;
    let (stdout, _stderr) = tool_chain.invoke_with_fallback(&batch_prompt, &mut logger)?;

    let content = fs::read_to_string(&checklist)?;
    let mut new_content = content;

    // Append generated items
    if !new_content.ends_with('\n') {
        new_content.push('\n');
    }
    new_content.push_str("\n# Generated Items\n");
    new_content.push_str(&stdout);

    fs::write(&checklist, new_content)?;
    println!("Added batch items to {}", checklist.display());

    Ok(())
}

pub fn cmd_remove(checklist: PathBuf, pattern: String, yes: bool) -> Result<()> {
    if !checklist.exists() {
        anyhow::bail!("Checklist file does not exist: {}", checklist.display());
    }

    let content = fs::read_to_string(&checklist)?;
    let lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();

    let matching_indices: Vec<(usize, String)> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.contains(&pattern))
        .map(|(i, line)| (i, line.clone()))
        .collect();

    if matching_indices.is_empty() {
        println!("No items match pattern: {}", pattern);
        return Ok(());
    }

    println!("Found {} matching item(s):", matching_indices.len());
    for (i, line) in &matching_indices {
        println!("  [{}] {}", i, line);
    }

    if !yes {
        print!("\nRemove these items? [y/N] ");
        io::stdout().flush()?;

        let mut response = String::new();
        io::stdin().read_line(&mut response)?;

        if !response.trim().eq_ignore_ascii_case("y") {
            println!("Cancelled.");
            return Ok(());
        }
    }

    let indices_to_remove: std::collections::HashSet<usize> =
        matching_indices.iter().map(|(i, _)| *i).collect();

    let filtered_lines: Vec<String> = lines
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !indices_to_remove.contains(i))
        .map(|(_, line)| line)
        .collect();

    let updated_content = filtered_lines.join("\n") + "\n";
    fs::write(&checklist, updated_content)?;

    println!("Removed {} item(s)", matching_indices.len());

    Ok(())
}

pub fn cmd_update(checklist: PathBuf, instruction: String, tools: String, model_config: ModelConfig) -> Result<()> {
    if !checklist.exists() {
        anyhow::bail!("Checklist file does not exist: {}", checklist.display());
    }

    let original_content = fs::read_to_string(&checklist)?;

    let checklist_path = checklist
        .to_str()
        .context("Checklist path contains invalid UTF-8")?;

    let update_prompt = format!(
        r#"@{}

{}

Update the checklist according to the instruction above. Output the COMPLETE updated checklist, preserving all standing orders and existing content. Make only the requested changes.
"#,
        checklist_path, instruction
    );

    println!("Updating checklist...");
    let mut tool_chain = LlmToolChain::with_models(&tools, &model_config)?;
    let mut logger = None;
    let (stdout, _stderr) = tool_chain.invoke_with_fallback(&update_prompt, &mut logger)?;

    // Verify standing orders are preserved
    let mut updated_content = stdout;
    if !updated_content.contains("# STANDING ORDERS") {
        // LLM removed standing orders, restore them
        println!("Warning: Standing orders were removed by LLM, restoring them...");

        // Extract title from original content
        let original_lines: Vec<&str> = original_content.lines().collect();
        let mut new_content = String::new();

        // Find and preserve the title
        if let Some(first_line) = original_lines.first() {
            if first_line.starts_with("# ") {
                new_content.push_str(first_line);
                new_content.push_str("\n\n");
            }
        }

        // Add standing orders
        let core_orders = render_core_standing_orders(DEFAULT_COMPLETION_TOKEN);
        new_content.push_str(&core_orders);
        new_content.push_str("\n\n# Project Standing Orders\n\n");

        // Add the LLM's content (skipping title if present)
        let llm_lines: Vec<&str> = updated_content.lines().collect();
        let mut skip_first = false;
        if let Some(first_line) = llm_lines.first() {
            if first_line.starts_with("# ") {
                skip_first = true;
            }
        }

        for (i, line) in llm_lines.iter().enumerate() {
            if skip_first && i == 0 {
                continue;
            }
            new_content.push_str(line);
            new_content.push('\n');
        }

        updated_content = new_content;
    }

    // Create backup
    let backup_path = checklist.with_extension("md.bak");
    fs::copy(&checklist, &backup_path)?;
    println!("Created backup: {}", backup_path.display());

    fs::write(&checklist, updated_content)?;
    println!("Updated checklist: {}", checklist.display());

    Ok(())
}
