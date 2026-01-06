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
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::NamedTempFile;

use crate::constants::{render_core_standing_orders, CORE_STANDING_ORDERS_VERSION, STANDING_ORDERS_AUDIT_PROMPT_TEMPLATE};
use crate::llm::LlmToolChain;
use crate::logger::Logger;
use crate::runner::{log_message, log_warning, stream_outputs};

pub enum AuditTarget {
    File {
        path: PathBuf,
        content: String,
    },
    ChecklistSection {
        checklist_path: PathBuf,
        checklist_content: String,
        section_start: usize,
        section_end: usize,
        section_content: String,
    },
}

pub struct AuditConfig<'a> {
    pub checklist: &'a PathBuf,
    pub completion_token: &'a str,
    pub audit_orders_path: &'a Option<PathBuf>,
    pub commit_audit: bool,
}

pub fn run_standing_orders_audit(
    config: &AuditConfig,
    tool_chain: &mut LlmToolChain,
    logger: &mut Option<Logger>,
) -> Result<()> {
    let target = resolve_audit_target(config)?;
    let (current_content, target_label) = match &target {
        AuditTarget::File { path, content } => (content.clone(), path.display().to_string()),
        AuditTarget::ChecklistSection {
            checklist_path,
            section_content,
            ..
        } => (
            section_content.clone(),
            format!("{} (Standing Orders section)", checklist_path.display()),
        ),
    };

    log_message(
        logger,
        &format!("Running Standing Orders audit targeting {}", target_label),
    );

    let core_orders = render_core_standing_orders(config.completion_token);

    let mut _orders_file_temp: Option<NamedTempFile> = None;
    let mut orders_current_temp = NamedTempFile::new().context("Failed to create temp file")?;
    orders_current_temp
        .write_all(current_content.as_bytes())
        .context("Failed to write current Standing Orders snapshot")?;
    orders_current_temp
        .flush()
        .context("Failed to flush Standing Orders snapshot")?;

    let orders_current_path = orders_current_temp.path().to_string_lossy().to_string();

    let orders_file_path = match &target {
        AuditTarget::File { path, .. } => {
            if !path.exists() {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)
                        .with_context(|| format!("Failed to create {}", parent.display()))?;
                }
                fs::write(path, &current_content)
                    .with_context(|| format!("Failed to initialize {}", path.display()))?;
            }
            path.to_string_lossy().to_string()
        }
        AuditTarget::ChecklistSection { .. } => {
            let mut temp = NamedTempFile::new().context("Failed to create temp file")?;
            temp.write_all(current_content.as_bytes())
                .context("Failed to write Standing Orders block")?;
            temp.flush()
                .context("Failed to flush Standing Orders block")?;
            let path = temp.path().to_string_lossy().to_string();
            _orders_file_temp = Some(temp);
            path
        }
    };

    let prompt_body = STANDING_ORDERS_AUDIT_PROMPT_TEMPLATE
        .replace(
            "{CORE_STANDING_ORDERS_WITH_TOKEN_SUBSTITUTED}",
            &core_orders,
        )
        .replace("{completion_token}", config.completion_token)
        .replace("{orders_file}", &orders_file_path)
        .replace("{orders_current}", &orders_current_path);

    let (stdout, stderr) = tool_chain.invoke_with_fallback(&prompt_body, logger)?;
    stream_outputs("audit", &stdout, &stderr, logger);

    let mut rendered = stdout;
    if !rendered.ends_with('\n') {
        rendered.push('\n');
    }

    let commit_path = match target {
        AuditTarget::File { path, .. } => {
            fs::write(&path, rendered)
                .with_context(|| format!("Failed to write {}", path.display()))?;
            path
        }
        AuditTarget::ChecklistSection {
            checklist_path,
            checklist_content,
            section_start,
            section_end,
            ..
        } => {
            let mut updated = String::with_capacity(checklist_content.len() + rendered.len());
            updated.push_str(&checklist_content[..section_start]);
            updated.push_str(&rendered);
            updated.push_str(&checklist_content[section_end..]);
            fs::write(&checklist_path, updated).with_context(|| {
                format!(
                    "Failed to update checklist {} with Standing Orders",
                    checklist_path.display()
                )
            })?;
            checklist_path
        }
    };

    maybe_commit_audit_change(&commit_path, config.commit_audit, logger)?;

    Ok(())
}

fn resolve_audit_target(config: &AuditConfig) -> Result<AuditTarget> {
    if let Some(path) = config.audit_orders_path {
        return load_file_audit_target(path);
    }

    let default_agents = PathBuf::from("AGENTS.md");
    if default_agents.exists() {
        return load_file_audit_target(&default_agents);
    }

    let checklist_content = fs::read_to_string(config.checklist).with_context(|| {
        format!(
            "Failed to read checklist for audit: {}",
            config.checklist.display()
        )
    })?;

    if let Some((start, end, section_content)) = extract_standing_orders_block(&checklist_content) {
        return Ok(AuditTarget::ChecklistSection {
            checklist_path: config.checklist.clone(),
            checklist_content,
            section_start: start,
            section_end: end,
            section_content,
        });
    }

    load_file_audit_target(&default_agents)
}

fn load_file_audit_target(path: &Path) -> Result<AuditTarget> {
    let content = if path.exists() {
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?
    } else {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        fs::write(path, "").with_context(|| format!("Failed to initialize {}", path.display()))?;
        String::new()
    };

    Ok(AuditTarget::File {
        path: path.to_path_buf(),
        content,
    })
}

fn extract_standing_orders_block(contents: &str) -> Option<(usize, usize, String)> {
    let header = "# STANDING ORDERS - DO NOT DELETE";
    let start = contents.find(header)?;

    let bytes = contents.as_bytes();
    let mut end = contents.len();
    let mut first_line = true;
    let mut idx = start;

    while idx < contents.len() {
        if bytes[idx] == b'\n' {
            let next_line_start = idx + 1;
            if next_line_start >= contents.len() {
                end = contents.len();
                break;
            }

            if first_line {
                first_line = false;
            } else if contents[next_line_start..].starts_with("# ")
                || contents[next_line_start..].starts_with("## ")
            {
                end = idx + 1;
                break;
            }
        }
        idx += 1;
    }

    let section = contents[start..end].to_string();
    Some((start, end, section))
}

fn maybe_commit_audit_change(
    path: &Path,
    commit_audit: bool,
    logger: &mut Option<Logger>,
) -> Result<()> {
    if !commit_audit {
        return Ok(());
    }

    let path_str = path.to_string_lossy().to_string();

    match Command::new("git").arg("add").arg(&path_str).status() {
        Ok(status) if status.success() => {}
        Ok(status) => {
            log_warning(
                logger,
                &format!(
                    "Warning: git add {} returned non-zero status: {}",
                    path_str, status
                ),
            );
            return Ok(());
        }
        Err(e) => {
            log_warning(
                logger,
                &format!("Warning: failed to run git add {}: {}", path_str, e),
            );
            return Ok(());
        }
    }

    let message = format!(
        "afkcode: align Standing Orders (core v{})",
        CORE_STANDING_ORDERS_VERSION
    );

    match Command::new("git")
        .arg("commit")
        .arg("-m")
        .arg(&message)
        .status()
    {
        Ok(status) if status.success() => {
            log_message(
                logger,
                &format!(
                    "Standing Orders alignment committed with message: {}",
                    message
                ),
            );
        }
        Ok(status) => {
            log_warning(
                logger,
                &format!(
                    "Warning: git commit returned status {} (likely no changes to commit)",
                    status
                ),
            );
        }
        Err(e) => {
            log_warning(
                logger,
                &format!("Warning: failed to run git commit for audit: {}", e),
            );
        }
    }

    Ok(())
}
