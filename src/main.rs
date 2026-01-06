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

mod audit;
mod cli;
mod commands;
mod config;
mod constants;
mod coordinator;
mod gimme;
mod llm;
mod logger;
mod parallel;
mod prompts;
mod runner;
mod wakelock;

use anyhow::{anyhow, Context, Result};
use clap::Parser;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cli::{Cli, Commands, RunMode};
use commands::*;
use config::Config;
use constants::{DEFAULT_COMPLETION_TOKEN, DEFAULT_CONTROLLER_PROMPT};
use llm::ModelConfig;

fn main() -> Result<()> {
    let shutdown_flag = Arc::new(AtomicBool::new(false));
    let last_sigint = Arc::new(Mutex::new(None::<Instant>));

    let shutdown_flag_clone = shutdown_flag.clone();
    let last_sigint_clone = last_sigint.clone();

    // Handle Ctrl+C gracefully
    ctrlc::set_handler(move || {
        let now = Instant::now();
        let mut last = last_sigint_clone.lock().unwrap();
        
        if let Some(t) = *last {
            if now.duration_since(t) < Duration::from_secs(5) {
                println!("\nInterrupted again. Force exiting.");
                std::process::exit(0);
            }
        }
        
        *last = Some(now);
        shutdown_flag_clone.store(true, Ordering::SeqCst);
        println!("\nInterrupted. Finishing current turn... (Press Ctrl+C again within 5s to force exit)");
    })
    .context("Error setting Ctrl-C handler")?;

    let cli = Cli::parse();

    // Load config from specified path or default afkcode.toml
    let config_path = cli
        .config
        .clone()
        .unwrap_or_else(|| PathBuf::from("afkcode.toml"));
    let config = Config::load(&config_path)?;

    match cli.command {
        Commands::Run {
            checklist,
            controller_prompt,
            worker_prompt,
            completion_token,
            sleep_seconds,
            mode,
            run_audit,
            audit_orders_path,
            no_commit_audit,
            tools,
            log_file,
            gemini_model,
            claude_model,
            codex_model,
            num_instances,
            warmup_delay,
            no_gimme,
            gimme_path,
            items_per_instance,
        } => {
            // Merge config with CLI args
            let merged_controller_prompt = config.merge_with_cli(
                controller_prompt.clone(),
                config.controller_prompt.clone(),
                DEFAULT_CONTROLLER_PROMPT.to_string(),
            );
            let merged_worker_prompt = config.merge_with_cli(
                worker_prompt.clone(),
                config.worker_prompt.clone(),
                prompts::DEFAULT_WORKER_PROMPT.to_string(),
            );
            let merged_completion_token = config.merge_with_cli(
                completion_token.clone(),
                config.completion_token.clone(),
                DEFAULT_COMPLETION_TOKEN.to_string(),
            );
            let merged_sleep_seconds =
                config.merge_with_cli(sleep_seconds, config.sleep_seconds, 15u64);
            // Audit is skipped by default unless --run-audit is passed or config says to run it
            let merged_skip_audit = if run_audit {
                false // --run-audit was passed, so don't skip
            } else if let Some(skip) = config.skip_audit {
                skip // Use config value
            } else {
                true // Default: skip audit
            };
            let merged_mode = if matches!(mode, RunMode::Worker) {
                if let Some(mode_str) = &config.mode {
                    mode_str.parse::<RunMode>().map_err(|err| anyhow!(err))?
                } else {
                    mode
                }
            } else {
                mode
            };
            let merged_audit_orders_path = if let Some(path) = audit_orders_path.clone() {
                Some(path)
            } else if let Some(config_path) = &config.orders_path {
                Some(PathBuf::from(config_path))
            } else {
                None
            };
            let merged_commit_audit = if no_commit_audit {
                false
            } else {
                config.commit_audit.unwrap_or(true)
            };
            let merged_tools = config.merge_with_cli(
                tools.clone(),
                config.tools.clone(),
                "gemini,codex,claude".to_string(),
            );
            let merged_log_file = config.merge_with_cli(
                log_file.clone(),
                config.log_file.clone(),
                "afkcode.log".to_string(),
            );

            // Merge model configurations (CLI takes precedence over config file)
            // For Warp Agent API key, try: CLI flag -> env var -> config file
            let warp_api_key = config.warp_api_key.clone()
                .or_else(|| std::env::var("WARP_API_KEY").ok());
            
            let model_config = ModelConfig {
                gemini_model: gemini_model.or(config.gemini_model.clone()),
                claude_model: claude_model.or(config.claude_model.clone()),
                codex_model: codex_model.or(config.codex_model.clone()),
                warp_model: config.warp_model.clone(),
                warp_api_key,
            };

            // Merge parallel/gimme settings
            let merged_num_instances =
                config.merge_with_cli(num_instances, config.num_instances, 1usize);
            let merged_warmup_delay =
                config.merge_with_cli(warmup_delay, config.warmup_delay, 30u64);
            let merged_gimme_enabled = if no_gimme {
                false
            } else {
                config.gimme_mode.unwrap_or(true)
            };
            let merged_gimme_base_path = gimme_path.unwrap_or_else(|| {
                config
                    .gimme_base_path
                    .as_ref()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("."))
            });
            let merged_items_per_instance =
                config.merge_with_cli(items_per_instance, config.gimme_items_per_instance, 1usize);

            cmd_run(
                checklist,
                merged_controller_prompt,
                merged_worker_prompt,
                merged_completion_token,
                merged_sleep_seconds,
                merged_mode,
                merged_skip_audit,
                merged_audit_orders_path,
                merged_commit_audit,
                merged_tools,
                merged_log_file,
                model_config,
                shutdown_flag,
                merged_num_instances,
                merged_warmup_delay,
                merged_gimme_enabled,
                merged_gimme_base_path,
                merged_items_per_instance,
            )
        }
        Commands::Init {
            checklist,
            title,
            examples,
        } => cmd_init(checklist, title, examples),
        Commands::Generate {
            checklist,
            prompt,
            tools,
            gemini_model,
            claude_model,
            codex_model,
        } => {
            let merged_tools = config.merge_with_cli(
                tools.clone(),
                config.tools.clone(),
                "gemini,codex,claude".to_string(),
            );
            let warp_api_key = config.warp_api_key.clone()
                .or_else(|| std::env::var("WARP_API_KEY").ok());
            let model_config = ModelConfig {
                gemini_model: gemini_model.or(config.gemini_model.clone()),
                claude_model: claude_model.or(config.claude_model.clone()),
                codex_model: codex_model.or(config.codex_model.clone()),
                warp_model: config.warp_model.clone(),
                warp_api_key,
            };
            cmd_generate(checklist, prompt, merged_tools, model_config)
        }
        Commands::Add {
            checklist,
            item,
            sub,
            section,
        } => cmd_add(checklist, item, sub, section),
        Commands::AddBatch {
            checklist,
            description,
            tools,
            gemini_model,
            claude_model,
            codex_model,
        } => {
            let merged_tools = config.merge_with_cli(
                tools.clone(),
                config.tools.clone(),
                "gemini,codex,claude".to_string(),
            );
            let warp_api_key = config.warp_api_key.clone()
                .or_else(|| std::env::var("WARP_API_KEY").ok());
            let model_config = ModelConfig {
                gemini_model: gemini_model.or(config.gemini_model.clone()),
                claude_model: claude_model.or(config.claude_model.clone()),
                codex_model: codex_model.or(config.codex_model.clone()),
                warp_model: config.warp_model.clone(),
                warp_api_key,
            };
            cmd_add_batch(checklist, description, merged_tools, model_config)
        }
        Commands::Remove {
            checklist,
            pattern,
            yes,
        } => cmd_remove(checklist, pattern, yes),
        Commands::Update {
            checklist,
            instruction,
            tools,
            gemini_model,
            claude_model,
            codex_model,
        } => {
            let merged_tools = config.merge_with_cli(
                tools.clone(),
                config.tools.clone(),
                "gemini,codex,claude".to_string(),
            );
            let warp_api_key = config.warp_api_key.clone()
                .or_else(|| std::env::var("WARP_API_KEY").ok());
            let model_config = ModelConfig {
                gemini_model: gemini_model.or(config.gemini_model.clone()),
                claude_model: claude_model.or(config.claude_model.clone()),
                codex_model: codex_model.or(config.codex_model.clone()),
                warp_model: config.warp_model.clone(),
                warp_api_key,
            };
            cmd_update(checklist, instruction, merged_tools, model_config)
        }
    }
}
