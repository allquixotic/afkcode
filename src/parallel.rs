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

//! Parallel LLM subprocess orchestration.
//!
//! Manages multiple LLM instances running in parallel with staggered
//! warmup delays to prevent API rate limiting.

use anyhow::Result;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::coordinator::{StopCoordinator, SubprocessResult};
use crate::gimme::{self, ChecklistItem, CheckoutFilters, CheckoutRequest};
use crate::llm::{LlmToolChain, ModelConfig};
use crate::logger::Logger;
use crate::runner::{self, RunConfig};

/// Configuration for parallel LLM execution.
#[derive(Debug, Clone)]
pub struct ParallelConfig {
    /// Number of parallel LLM instances to run.
    pub num_instances: usize,
    /// Delay between launching each instance (0 = no delay).
    pub warmup_delay: Duration,
    /// Enable gimme mode for work item checkout.
    pub gimme_enabled: bool,
    /// Base path for AGENTS.md file search.
    pub gimme_base_path: PathBuf,
    /// Number of work items each instance should check out.
    pub items_per_instance: usize,
    /// Base run configuration (shared settings).
    pub run_config: RunConfig,
    /// Model configuration for tool chains.
    pub model_config: ModelConfig,
    /// Tools string (comma-separated).
    pub tools: String,
    /// Base log file path.
    pub log_file: String,
}

/// Run multiple LLM instances in parallel.
///
/// Launches instances with staggered warmup delays. Each instance has its
/// own independent LlmToolChain for fallback tracking. If any instance
/// confirms stop, all others finish their current iteration and exit.
pub fn run_parallel(config: ParallelConfig) -> Result<()> {
    let coordinator = Arc::new(StopCoordinator::new(config.num_instances));
    let mut handles: Vec<(usize, JoinHandle<Result<SubprocessResult>>)> = Vec::new();

    println!(
        "Starting {} parallel LLM instances with {}s warmup delay",
        config.num_instances,
        config.warmup_delay.as_secs()
    );

    // Launch subprocesses with staggered warmup
    for id in 0..config.num_instances {
        // Wait warmup_delay before launching (except first)
        if id > 0 && config.warmup_delay > Duration::ZERO {
            println!(
                "Waiting {}s before launching instance {}...",
                config.warmup_delay.as_secs(),
                id
            );

            // Check periodically during warmup in case stop is signaled
            let warmup_start = std::time::Instant::now();
            while warmup_start.elapsed() < config.warmup_delay {
                if coordinator.should_stop() || config.run_config.shutdown_flag.load(Ordering::Relaxed) {
                    println!("Stop signaled during warmup, not launching instance {}", id);
                    break;
                }
                thread::sleep(Duration::from_millis(100));
            }

            if coordinator.should_stop() || config.run_config.shutdown_flag.load(Ordering::Relaxed) {
                break;
            }
        }

        // Check if already stopping
        if coordinator.should_stop() || config.run_config.shutdown_flag.load(Ordering::Relaxed) {
            println!("Stop signaled before launching instance {}", id);
            break;
        }

        // Checkout work items if gimme mode enabled
        let work_items = if config.gimme_enabled {
            match checkout_work_items(&config, id) {
                Ok(items) => {
                    if !items.is_empty() {
                        println!(
                            "Instance {} checked out {} work item(s)",
                            id,
                            items.len()
                        );
                    }
                    items
                }
                Err(e) => {
                    eprintln!("Warning: Failed to checkout work items for instance {}: {}", id, e);
                    vec![]
                }
            }
        } else {
            vec![]
        };

        // Create independent LlmToolChain for this subprocess
        let tool_chain = LlmToolChain::with_models(&config.tools, &config.model_config)?;

        // Create independent logger with subprocess ID in filename
        let log_file = format!("{}.{}", config.log_file, id);
        let logger = Logger::new(&log_file).ok();

        // Spawn subprocess thread
        let handle = spawn_subprocess(
            id,
            tool_chain,
            logger,
            work_items,
            config.run_config.clone(),
            coordinator.clone(),
            config.gimme_enabled,
        );

        handles.push((id, handle));
        println!("Launched instance {}", id);
    }

    // Wait for stop signal or all threads to complete
    loop {
        // Check if any thread signaled stop
        if coordinator.should_stop() {
            println!("Stop confirmed. Waiting for all instances to finish current iteration...");
            coordinator.wait_for_all_complete(Duration::from_secs(300));
            break;
        }

        // Check for Ctrl+C
        if config.run_config.shutdown_flag.load(Ordering::Relaxed) {
            println!("Shutdown requested. Waiting for instances to finish...");
            coordinator.wait_for_all_complete(Duration::from_secs(60));
            break;
        }

        // Check if all threads completed
        let all_done = handles.iter().all(|(id, _)| coordinator.is_completed(*id));
        if all_done {
            break;
        }

        thread::sleep(Duration::from_millis(100));
    }

    // Join all threads and collect results
    for (id, handle) in handles {
        match handle.join() {
            Ok(Ok(result)) => {
                println!("Instance {} completed: {:?}", id, result);
            }
            Ok(Err(e)) => {
                eprintln!("Instance {} error: {}", id, e);
            }
            Err(_) => {
                eprintln!("Instance {} thread panicked", id);
            }
        }
    }

    Ok(())
}

/// Checkout work items for a subprocess.
fn checkout_work_items(config: &ParallelConfig, subprocess_id: usize) -> Result<Vec<ChecklistItem>> {
    let request = CheckoutRequest {
        num_items: config.items_per_instance,
        base_path: config.gimme_base_path.clone(),
        filters: CheckoutFilters {
            incomplete: true,
            unverified: false,
            blocked: false,
        },
    };

    let result = gimme::checkout::checkout(request, subprocess_id)?;
    Ok(result.items)
}

/// Spawn a subprocess thread.
fn spawn_subprocess(
    id: usize,
    mut tool_chain: LlmToolChain,
    mut logger: Option<Logger>,
    work_items: Vec<ChecklistItem>,
    run_config: RunConfig,
    coordinator: Arc<StopCoordinator>,
    gimme_enabled: bool,
) -> JoinHandle<Result<SubprocessResult>> {
    thread::spawn(move || {
        let result = runner::run_worker_loop_parallel(
            &run_config,
            &mut tool_chain,
            &mut logger,
            &coordinator,
            id,
            &work_items,
        );

        // Handle result
        match &result {
            Ok(r) => {
                coordinator.mark_completed(id, r.clone());
            }
            Err(e) => {
                // On error, try to restore work items if gimme mode was enabled
                if gimme_enabled && !work_items.is_empty() {
                    for item in &work_items {
                        if let Err(restore_err) = gimme::marker::restore_item(item) {
                            eprintln!(
                                "Warning: Failed to restore work item for instance {}: {}",
                                id, restore_err
                            );
                        }
                    }
                }
                coordinator.mark_completed(id, SubprocessResult::Error(e.to_string()));
            }
        }

        result
    })
}
