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

use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

use crate::constants::{DEFAULT_COMPLETION_TOKEN, DEFAULT_CONTROLLER_PROMPT};
use crate::prompts;

#[derive(Parser)]
#[command(name = "afkcode")]
#[command(about = "LLM-powered checklist management and autonomous development loop")]
#[command(version)]
pub struct Cli {
    /// Path to config file (defaults to afkcode.toml in current directory if it exists)
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub enum RunMode {
    Worker,
    Controller,
}

impl std::str::FromStr for RunMode {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "worker" => Ok(Self::Worker),
            "controller" => Ok(Self::Controller),
            other => Err(format!("Invalid run mode: {}", other)),
        }
    }
}

#[derive(Subcommand)]
pub enum Commands {
    /// Run the controller/worker loop against a checklist
    Run {
        /// Path to the persistent checklist file
        checklist: PathBuf,

        /// Custom controller prompt template
        #[arg(long, default_value = DEFAULT_CONTROLLER_PROMPT)]
        controller_prompt: String,

        /// Custom worker prompt template
        #[arg(long, default_value = prompts::DEFAULT_WORKER_PROMPT)]
        worker_prompt: String,

        /// Completion detection token
        #[arg(long, default_value = DEFAULT_COMPLETION_TOKEN)]
        completion_token: String,

        /// Delay between LLM invocations in seconds
        #[arg(long, default_value_t = 15)]
        sleep_seconds: u64,

        /// Loop mode (worker-only default, or controller/worker alternation)
        #[arg(long, value_enum, default_value_t = RunMode::Worker)]
        mode: RunMode,

        /// Run the standing orders audit on startup (disabled by default)
        #[arg(long)]
        run_audit: bool,

        /// Override the target file used for the standing orders audit
        #[arg(long)]
        audit_orders_path: Option<PathBuf>,

        /// Disable committing the audit changes automatically
        #[arg(long, hide = true)]
        no_commit_audit: bool,

        /// Comma-separated list of LLM tools to try (gemini, codex, claude)
        #[arg(long, default_value = "gemini,codex,claude")]
        tools: String,

        /// Log file path for streaming output
        #[arg(long, default_value = "afkcode.log")]
        log_file: String,

        /// Model to use for Gemini CLI (e.g., gemini-2.5-pro)
        #[arg(long)]
        gemini_model: Option<String>,

        /// Model to use for Claude CLI (e.g., sonnet, opus, claude-sonnet-4-5-20250929)
        #[arg(long)]
        claude_model: Option<String>,

        /// Model to use for Codex CLI (e.g., o3, o4-mini)
        #[arg(long)]
        codex_model: Option<String>,

        /// Number of parallel LLM instances
        #[arg(long, default_value_t = 1)]
        num_instances: usize,

        /// Warmup delay between instances in seconds (0 to disable)
        #[arg(long, default_value_t = 30)]
        warmup_delay: u64,

        /// Disable gimme mode (work item checkout)
        #[arg(long)]
        no_gimme: bool,

        /// Base path for AGENTS.md file search
        #[arg(long)]
        gimme_path: Option<PathBuf>,

        /// Number of work items per instance
        #[arg(long, default_value_t = 1)]
        items_per_instance: usize,
    },

    /// Initialize a new bare checklist with standing orders
    Init {
        /// Path to create the checklist file
        checklist: PathBuf,

        /// Title for the checklist
        #[arg(short, long)]
        title: Option<String>,

        /// Include example sections
        #[arg(short, long)]
        examples: bool,
    },

    /// Generate a checklist from a high-level prompt using LLM
    Generate {
        /// Path to create the checklist file
        checklist: PathBuf,

        /// High-level description of what to build
        prompt: String,

        /// Comma-separated list of LLM tools to try
        #[arg(long, default_value = "gemini,gemini,codex,claude")]
        tools: String,

        /// Model to use for Gemini CLI
        #[arg(long)]
        gemini_model: Option<String>,

        /// Model to use for Claude CLI
        #[arg(long)]
        claude_model: Option<String>,

        /// Model to use for Codex CLI
        #[arg(long)]
        codex_model: Option<String>,
    },

    /// Add a single item to the checklist
    Add {
        /// Path to the checklist file
        checklist: PathBuf,

        /// Item text to add
        item: String,

        /// Add as a sub-item (indented)
        #[arg(short, long)]
        sub: bool,

        /// Section to add to (default: end of file)
        #[arg(short = 's', long)]
        section: Option<String>,
    },

    /// Add multiple items using LLM to expand a high-level description
    AddBatch {
        /// Path to the checklist file
        checklist: PathBuf,

        /// High-level description of items to add
        description: String,

        /// Comma-separated list of LLM tools to try
        #[arg(long, default_value = "gemini,codex,claude")]
        tools: String,

        /// Model to use for Gemini CLI
        #[arg(long)]
        gemini_model: Option<String>,

        /// Model to use for Claude CLI
        #[arg(long)]
        claude_model: Option<String>,

        /// Model to use for Codex CLI
        #[arg(long)]
        codex_model: Option<String>,
    },

    /// Remove items from the checklist
    Remove {
        /// Path to the checklist file
        checklist: PathBuf,

        /// Pattern to match for removal (substring match)
        pattern: String,

        /// Confirm removal without prompting
        #[arg(short, long)]
        yes: bool,
    },

    /// Update/maintain checklist items using LLM
    Update {
        /// Path to the checklist file
        checklist: PathBuf,

        /// Instructions for updating the checklist
        instruction: String,

        /// Comma-separated list of LLM tools to try
        #[arg(long, default_value = "gemini,codex,claude")]
        tools: String,

        /// Model to use for Gemini CLI
        #[arg(long)]
        gemini_model: Option<String>,

        /// Model to use for Claude CLI
        #[arg(long)]
        claude_model: Option<String>,

        /// Model to use for Codex CLI
        #[arg(long)]
        codex_model: Option<String>,
    },
}
