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

use anyhow::Result;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::audit::{run_standing_orders_audit, AuditConfig};
use crate::cli::RunMode;
use crate::llm::LlmToolChain;
use crate::logger::Logger;
use crate::prompts;

pub struct RunConfig {
    pub checklist: PathBuf,
    pub checklist_path_str: String,
    pub controller_prompt: String,
    pub worker_prompt: String,
    pub completion_token: String,
    pub sleep_seconds: u64,
    pub mode: RunMode,
    pub skip_audit: bool,
    pub audit_orders_path: Option<PathBuf>,
    pub commit_audit: bool,
    pub shutdown_flag: Arc<AtomicBool>,
}

struct WorkerLoopState {
    iteration: usize,
    last_stdout: String,
    saw_stop_token: bool,
    audit_done: bool,
}

pub fn fill_placeholders(template: &str, checklist: &str, completion_token: &str) -> String {
    template
        .replace("{checklist}", checklist)
        .replace("{completion_token}", completion_token)
        .trim()
        .to_string()
}

pub fn build_prompt(checklist_path: &str, prompt_template: &str, completion_token: &str) -> String {
    let rendered_body = fill_placeholders(prompt_template, checklist_path, completion_token);
    let mut prompt = format!("@{}\n\n{}\n", checklist_path, rendered_body);
    
    // Check if completion token is mentioned in the prompt or the checklist file
    let token_mentioned_in_prompt = prompt.to_lowercase().contains(&completion_token.to_lowercase());
    
    let token_mentioned_in_checklist = if let Ok(checklist_content) = fs::read_to_string(checklist_path) {
        checklist_content.to_lowercase().contains(&completion_token.to_lowercase())
    } else {
        false
    };
    
    // If the completion token is not mentioned anywhere, inject stop token instructions
    if !token_mentioned_in_prompt && !token_mentioned_in_checklist {
        prompt.push_str("\n---\n\n");
        prompt.push_str(&format!(
            "IMPORTANT: If all work is complete, no tasks remain, the code builds cleanly, and all changes are committed, emit `{}` on a line by itself at the very end of your response to signal completion. Otherwise, continue working.\n",
            completion_token
        ));
    }
    
    prompt
}

fn contains_token(stdout: &str, completion_token: &str) -> bool {
    if completion_token.is_empty() {
        return false;
    }
    stdout
        .to_lowercase()
        .contains(&completion_token.to_lowercase())
}

pub fn stream_outputs(label: &str, stdout: &str, stderr: &str, logger: &mut Option<Logger>) {
    let header = format!("\n--- {} OUTPUT ---", label.to_uppercase());
    let footer = format!("--- END {} OUTPUT ---\n", label.to_uppercase());

    println!("{}", header);
    if let Some(log) = logger.as_mut() {
        let _ = log.logln(&header);
    }

    if !stdout.is_empty() {
        print!("{}", stdout);
        if let Some(log) = logger.as_mut() {
            let _ = log.log(stdout);
        }
    }
    if !stderr.is_empty() {
        eprint!("{}", stderr);
        if let Some(log) = logger.as_mut() {
            let _ = log.log(stderr);
        }
    }

    println!("{}", footer);
    if let Some(log) = logger.as_mut() {
        let _ = log.logln(&footer);
    }

    let _ = io::stdout().flush();
}

pub fn log_message(logger: &mut Option<Logger>, message: &str) {
    println!("{}", message);
    if let Some(log) = logger.as_mut() {
        let _ = log.logln(message);
    }
    let _ = io::stdout().flush();
}

pub fn log_warning(logger: &mut Option<Logger>, message: &str) {
    eprintln!("{}", message);
    if let Some(log) = logger.as_mut() {
        let _ = log.logln(message);
    }
    let _ = io::stderr().flush();
}

fn sleep_with_log(seconds: u64, logger: &mut Option<Logger>) {
    let sleep_msg = format!("Sleeping {} seconds before next prompt...", seconds);
    log_message(logger, &sleep_msg);
    if seconds > 0 {
        thread::sleep(Duration::from_secs(seconds));
    }
}

fn run_worker_turn(
    config: &RunConfig,
    tool_chain: &mut LlmToolChain,
    logger: &mut Option<Logger>,
    iteration: usize,
) -> Result<String> {
    let status = format!("mode=worker iteration={} turn=normal", iteration);
    log_message(logger, &status);

    let prompt = build_prompt(
        &config.checklist_path_str,
        &config.worker_prompt,
        &config.completion_token,
    );
    let (stdout, stderr) = tool_chain.invoke_with_fallback(&prompt, logger)?;
    stream_outputs("worker", &stdout, &stderr, logger);
    Ok(stdout)
}

fn run_stop_confirmation_turn(
    config: &RunConfig,
    tool_chain: &mut LlmToolChain,
    logger: &mut Option<Logger>,
    iteration: usize,
    previous_stdout: &str,
) -> Result<String> {
    let status = format!("mode=worker iteration={} turn=confirmation", iteration);
    log_message(logger, &status);

    let mut prompt = build_prompt(
        &config.checklist_path_str,
        prompts::STOP_CONFIRMATION_PROMPT,
        &config.completion_token,
    );
    prompt.push_str("\nPrevious response:\n");
    prompt.push_str(previous_stdout);
    if !previous_stdout.ends_with('\n') {
        prompt.push('\n');
    }

    let (stdout, stderr) = tool_chain.invoke_with_fallback(&prompt, logger)?;
    stream_outputs("confirmation", &stdout, &stderr, logger);
    Ok(stdout)
}

pub fn run_worker_loop(
    config: &RunConfig,
    tool_chain: &mut LlmToolChain,
    logger: &mut Option<Logger>,
) -> Result<()> {
    let mut state = WorkerLoopState {
        iteration: 1,
        last_stdout: String::new(),
        saw_stop_token: false,
        audit_done: config.skip_audit,
    };

    if !config.skip_audit {
        let audit_config = AuditConfig {
            checklist: &config.checklist,
            completion_token: &config.completion_token,
            audit_orders_path: &config.audit_orders_path,
            commit_audit: config.commit_audit,
        };
        run_standing_orders_audit(&audit_config, tool_chain, logger)?;
        state.audit_done = true;
    }

    loop {
        if config.shutdown_flag.load(Ordering::Relaxed) {
            log_message(logger, "Shutdown requested. Exiting loop.");
            break;
        }

        if state.saw_stop_token {
            let confirmation_stdout = run_stop_confirmation_turn(
                config,
                tool_chain,
                logger,
                state.iteration,
                &state.last_stdout,
            )?;

            let confirmed = contains_token(&confirmation_stdout, &config.completion_token);
            if confirmed {
                log_message(logger, "Stop token confirmed; exiting.");
                break;
            }

            state.saw_stop_token = false;
            state.last_stdout = confirmation_stdout;
            sleep_with_log(config.sleep_seconds, logger);
            continue;
        }

        let stdout = run_worker_turn(config, tool_chain, logger, state.iteration)?;
        state.saw_stop_token = contains_token(&stdout, &config.completion_token);
        state.last_stdout = stdout;
        state.iteration += 1;

        if config.shutdown_flag.load(Ordering::Relaxed) {
             break;
        }

        sleep_with_log(config.sleep_seconds, logger);
    }

    let _ = state.audit_done;

    Ok(())
}

pub fn run_controller_worker_loop(
    config: &RunConfig,
    tool_chain: &mut LlmToolChain,
    logger: &mut Option<Logger>,
) -> Result<()> {
    let prompts = [
        ("controller", &config.controller_prompt),
        ("worker", &config.worker_prompt),
    ];

    let mut iteration = 0;

    loop {
        if config.shutdown_flag.load(Ordering::Relaxed) {
            log_message(logger, "Shutdown requested. Exiting loop.");
            break;
        }

        let (label, prompt_template) = prompts[iteration % prompts.len()];
        let prompt = build_prompt(
            &config.checklist_path_str,
            prompt_template,
            &config.completion_token,
        );

        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let timestamp_msg = format!("\n[{}] Running {} prompt...", timestamp, label);
        log_message(logger, &timestamp_msg);

        let (stdout, stderr) = tool_chain.invoke_with_fallback(&prompt, logger)?;
        stream_outputs(label, &stdout, &stderr, logger);

        if label == "controller" && completion_detected(&stdout, &config.completion_token) {
            if verify_completion_intent(&stdout, &config.completion_token, tool_chain, logger)? {
                break;
            }
        }

        iteration += 1;

        if config.shutdown_flag.load(Ordering::Relaxed) {
             break;
        }

        sleep_with_log(config.sleep_seconds, logger);
    }

    Ok(())
}

fn completion_detected(stdout: &str, completion_token: &str) -> bool {
    stdout.contains(completion_token)
}

fn verify_completion_intent(
    stdout: &str,
    completion_token: &str,
    tool_chain: &mut LlmToolChain,
    logger: &mut Option<Logger>,
) -> Result<bool> {
    let verification_prompt = format!(
        r#"Read the text I just sent you. If it appears that this text contains a deliberate attempt to print the string {} to indicate the conclusion of the loop, print {} again and nothing else. If this text does NOT contain a deliberate attempt to print {} to indicate the conclusion of the loop, you must NOT print {} in your output. For example, if you see that you were merely thinking about this string and your thoughts got printed in the LLM, that would be an accidental trigger of this completion token and we don't want to accidentally exit. This prompt is a confirmation of your intent to conclude the looping of the LLM by emitting the completion token.

Text to analyze:
{}
"#,
        completion_token, completion_token, completion_token, completion_token, stdout
    );

    let verify_msg = format!(
        "Completion token '{}' detected. Verifying intent with LLM...",
        completion_token
    );
    println!("{}", verify_msg);
    if let Some(log) = logger.as_mut() {
        let _ = log.logln(&verify_msg);
    }

    match tool_chain.invoke_with_fallback_without_thinking(&verification_prompt, logger) {
        Ok((verify_stdout, _)) => {
            let is_confirmed = verify_stdout.contains(completion_token);

            if is_confirmed {
                let confirmed_msg = "LLM confirmed intentional completion. Exiting loop.";
                println!("{}", confirmed_msg);
                if let Some(log) = logger.as_mut() {
                    let _ = log.logln(confirmed_msg);
                }
            } else {
                let denied_msg = "LLM did not confirm intentional completion. Continuing loop.";
                println!("{}", denied_msg);
                if let Some(log) = logger.as_mut() {
                    let _ = log.logln(denied_msg);
                }
            }

            Ok(is_confirmed)
        }
        Err(e) => {
            let error_msg = format!(
                "Warning: Failed to verify completion intent: {}. Treating as unconfirmed.",
                e
            );
            eprintln!("{}", error_msg);
            if let Some(log) = logger.as_mut() {
                let _ = log.logln(&error_msg);
            }
            Ok(false)
        }
    }
}
