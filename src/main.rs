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

mod prompts;

use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::NamedTempFile;

const DEFAULT_COMPLETION_TOKEN: &str = "__ALL_TASKS_COMPLETE__";

const DEFAULT_CONTROLLER_PROMPT: &str = r#"
You are the controller in an autonomous development loop.
Study the shared checklist in @{checklist}, and reduce the length of it by removing completely finished checklist items.
If and only if all high-level requirements and every checklist item are fully satisfied, output {completion_token} on a line by itself at the very end of your reply; otherwise, do not print that string.
"#;

const CORE_STANDING_ORDERS_VERSION: &str = "1";

const CORE_STANDING_ORDERS_TEMPLATE: &str = r#"# STANDING ORDERS - DO NOT DELETE

1. Minimal Information: Checklist items contain only the minimum needed for an LLM to act.
2. Completion Handling: Delete fully complete items. For partials, change `[ ]` to `[~]` and add sub-items for the remaining work.
3. Discovery: Add newly discovered work as new (sub-)items, succinctly.
4. Git Commit: Before finishing a work turn, run `git add` and `git commit` with a descriptive message summarizing changes.
5. Immutability: The "STANDING ORDERS" section is immutable except during the one-time alignment step run by afkcode.
6. No Manual Work: Do not require or mention manual human steps or manual testing; prefer automated tests and processes.
7. "Do the thing": Review checklist, pick an important incomplete item, implement fully or partially, update checklist, build, fix errors, commit.
8. "Fix shit": Identify broken code/design or incomplete implementations, fix, update checklist, commit.
9. Stop Token Etiquette (Worker Mode): Emit `{completion_token}` on a line by itself at the very end ONLY when all requirements are met, no `[ ]` or `[~]` remain, the code builds cleanly, and all changes are committed.
"#;

const STANDING_ORDERS_AUDIT_PROMPT_TEMPLATE: &str = r#"You are aligning the repository's Standing Orders for afkcode's worker-only mode.

Goals:
- Make the Standing Orders section contain the immutable "core orders" shown below verbatim, followed by any project-specific orders that are relevant to THIS codebase (naming conventions, CI rules, code style, etc.).
- Remove duplicates and obsolete or vague rules. Keep each bullet concise and actionable.
- Project-specific additions must NOT contradict the core orders.

Core orders (must appear exactly as provided, with {completion_token} expanded to the actual token):
{CORE_STANDING_ORDERS_WITH_TOKEN_SUBSTITUTED}

Input file: @{orders_file}   # This is either AGENTS.md or the Standing Orders block inside the checklist.
Current contents of the target Standing Orders (or an empty placeholder if not present):
@{orders_current}

Instructions:
1) Replace the Standing Orders in the target file so that it begins with the exact core orders above (with the correct completion token inserted), followed by a heading `# Project Standing Orders` (create it if missing) and any project-specific orders you retain or add.
2) Do not include explanations, rationales, or commentary—ONLY the final file content.
3) Ensure formatting is valid Markdown and indentation is 4 spaces for sub-items where applicable.

Output ONLY the full updated file content (no prose).
"#;

/// Configuration file structure
#[derive(Debug, Default, Deserialize, Serialize)]
struct Config {
    /// LLM tools to use (comma-separated: codex, claude)
    tools: Option<String>,

    /// Sleep duration between LLM calls (in seconds)
    sleep_seconds: Option<u64>,

    /// Controller prompt template
    controller_prompt: Option<String>,

    /// Worker prompt template
    worker_prompt: Option<String>,

    /// Completion detection token
    completion_token: Option<String>,

    /// Log file path for streaming output
    log_file: Option<String>,

    /// Run mode (worker or controller)
    mode: Option<String>,

    /// Optional path override for standing orders audit
    orders_path: Option<String>,

    /// Skip the standing orders audit on startup
    skip_audit: Option<bool>,

    /// Commit audit changes automatically
    commit_audit: Option<bool>,
}

impl Config {
    /// Load config from a file, or return default if file doesn't exist
    fn load(path: &PathBuf) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;

        let config: Config = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))?;

        Ok(config)
    }

    /// Merge this config with CLI args, where CLI args take precedence
    fn merge_with_cli<T>(&self, cli_value: T, config_value: Option<T>, default_value: T) -> T
    where
        T: PartialEq + Clone,
    {
        // If CLI value differs from default, use CLI value
        if cli_value != default_value {
            cli_value
        } else if let Some(config_val) = config_value {
            // Otherwise use config value if present
            config_val
        } else {
            // Fall back to default
            default_value
        }
    }
}

/// LLM tool configuration and invocation logic
#[derive(Debug, Clone)]
enum LlmTool {
    Codex,
    Claude,
}

impl LlmTool {
    fn from_name(name: &str) -> Result<Self> {
        match name.to_lowercase().as_str() {
            "codex" => Ok(Self::Codex),
            "claude" => Ok(Self::Claude),
            _ => anyhow::bail!("Unsupported LLM tool: {}. Supported: codex, claude", name),
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
        }
    }

    fn command(&self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
        }
    }

    fn args(&self) -> Vec<String> {
        match self {
            Self::Codex => vec!["exec".to_string()],
            Self::Claude => vec![
                "--print".to_string(),
                "--dangerously-skip-permissions".to_string(),
            ],
        }
    }

    fn rate_limit_patterns(&self) -> Vec<&'static str> {
        match self {
            Self::Codex => vec![
                "rate limit reached",
                "rate_limit_error",
                "429",
                "too many requests",
            ],
            Self::Claude => vec![
                "usage limit reached",
                "rate limit reached",
                "rate_limit_error",
                "429",
                "limit will reset",
            ],
        }
    }

    fn is_rate_limited(&self, stdout: &str, stderr: &str) -> bool {
        let combined = format!("{}{}", stdout, stderr).to_lowercase();
        self.rate_limit_patterns()
            .iter()
            .any(|pattern| combined.contains(pattern))
    }

    fn invoke(&self, prompt: &str) -> Result<(String, String)> {
        let mut child = Command::new(self.command())
            .args(self.args())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| {
                format!(
                    "Failed to spawn {} process. Is {} CLI installed?",
                    self.name(),
                    self.name()
                )
            })?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(prompt.as_bytes())?;
        }

        let output = child.wait_with_output()?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        Ok((stdout, stderr))
    }

    /// Invoke the LLM with thinking disabled for simple verification tasks
    fn invoke_without_thinking(&self, prompt: &str) -> Result<(String, String)> {
        match self {
            Self::Claude => {
                // For Claude Code, wrap the prompt with thinking_mode disabled
                let wrapped_prompt =
                    format!("<thinking_mode>disabled</thinking_mode>\n\n{}", prompt);
                self.invoke(&wrapped_prompt)
            }
            Self::Codex => {
                // For Codex CLI, use minimal reasoning effort
                let mut args = self.args();
                args.push("-c".to_string());
                args.push("model_reasoning_effort=\"minimal\"".to_string());

                let mut child = Command::new(self.command())
                    .args(args)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .with_context(|| {
                        format!(
                            "Failed to spawn {} process. Is {} CLI installed?",
                            self.name(),
                            self.name()
                        )
                    })?;

                if let Some(mut stdin) = child.stdin.take() {
                    stdin.write_all(prompt.as_bytes())?;
                }

                let output = child.wait_with_output()?;
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();

                Ok((stdout, stderr))
            }
        }
    }
}

/// Manages multiple LLM tools with automatic fallback
struct LlmToolChain {
    tools: Vec<LlmTool>,
    current_index: usize,
    rate_limit_timestamps: HashMap<String, Instant>,
    rate_limit_timeout: Duration,
}

impl LlmToolChain {
    fn new(tool_names: &str) -> Result<Self> {
        let tools: Result<Vec<_>> = tool_names
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(LlmTool::from_name)
            .collect();

        let tools = tools?;

        if tools.is_empty() {
            anyhow::bail!("No valid LLM tools specified");
        }

        Ok(Self {
            tools,
            current_index: 0,
            rate_limit_timestamps: HashMap::new(),
            rate_limit_timeout: Duration::from_secs(300), // 5 minutes
        })
    }

    fn current_tool(&self) -> &LlmTool {
        &self.tools[self.current_index]
    }

    fn has_fallback(&self) -> bool {
        self.current_index < self.tools.len() - 1
    }

    fn switch_to_next(&mut self) -> Option<&LlmTool> {
        if self.has_fallback() {
            self.current_index += 1;
            Some(self.current_tool())
        } else {
            None
        }
    }

    /// Mark a tool as rate limited with current timestamp
    fn mark_rate_limited(&mut self, tool: &LlmTool) {
        self.rate_limit_timestamps
            .insert(tool.name().to_string(), Instant::now());
    }

    /// Check if a tool's rate limit has expired
    fn is_rate_limit_expired(&self, tool: &LlmTool) -> bool {
        if let Some(timestamp) = self.rate_limit_timestamps.get(tool.name()) {
            timestamp.elapsed() >= self.rate_limit_timeout
        } else {
            true // No rate limit recorded
        }
    }

    /// Try to reset to the most preferred available tool
    fn try_reset_to_preferred(&mut self, logger: &mut Option<Logger>) {
        // Check all tools starting from the most preferred
        for (index, tool) in self.tools.iter().enumerate() {
            if index < self.current_index && self.is_rate_limit_expired(tool) {
                let reset_msg = format!(
                    "Rate limit timeout expired for {}. Resetting to preferred tool.",
                    tool.name()
                );
                println!("{}", reset_msg);
                if let Some(log) = logger.as_mut() {
                    let _ = log.logln(&reset_msg);
                }
                self.current_index = index;
                return;
            }
        }
    }

    fn invoke_with_fallback(
        &mut self,
        prompt: &str,
        logger: &mut Option<Logger>,
    ) -> Result<(String, String)> {
        // Try to reset to a more preferred tool if rate limit has expired
        self.try_reset_to_preferred(logger);

        loop {
            let tool = self.current_tool().clone();
            let tool_msg = format!("Using LLM tool: {}", tool.name());
            println!("{}", tool_msg);
            if let Some(log) = logger.as_mut() {
                let _ = log.logln(&tool_msg);
            }

            match tool.invoke(prompt) {
                Ok((stdout, stderr)) => {
                    if tool.is_rate_limited(&stdout, &stderr) {
                        // Mark this tool as rate limited
                        self.mark_rate_limited(&tool);

                        let rate_limit_msg = format!(
                            "Rate limit detected for {}. Temporarily squelching for 5 minutes.",
                            tool.name()
                        );
                        println!("{}", rate_limit_msg);
                        if let Some(log) = logger.as_mut() {
                            let _ = log.logln(&rate_limit_msg);
                        }

                        if let Some(next_tool) = self.switch_to_next() {
                            let switch_msg =
                                format!("Switching to fallback tool: {}", next_tool.name());
                            println!("{}", switch_msg);
                            if let Some(log) = logger.as_mut() {
                                let _ = log.logln(&switch_msg);
                            }
                            continue;
                        } else {
                            anyhow::bail!("All LLM tools exhausted due to rate limits");
                        }
                    }

                    return Ok((stdout, stderr));
                }
                Err(e) => {
                    let error_msg = format!("Error invoking {}: {}", tool.name(), e);
                    println!("{}", error_msg);
                    if let Some(log) = logger.as_mut() {
                        let _ = log.logln(&error_msg);
                    }

                    if let Some(next_tool) = self.switch_to_next() {
                        let switch_msg =
                            format!("Switching to fallback tool: {}", next_tool.name());
                        println!("{}", switch_msg);
                        if let Some(log) = logger.as_mut() {
                            let _ = log.logln(&switch_msg);
                        }
                        continue;
                    } else {
                        return Err(e);
                    }
                }
            }
        }
    }

    /// Invoke with fallback, but with thinking disabled for simple verification tasks
    fn invoke_with_fallback_without_thinking(
        &mut self,
        prompt: &str,
        logger: &mut Option<Logger>,
    ) -> Result<(String, String)> {
        // Try to reset to a more preferred tool if rate limit has expired
        self.try_reset_to_preferred(logger);

        loop {
            let tool = self.current_tool().clone();
            let tool_msg = format!("Using LLM tool: {} (thinking disabled)", tool.name());
            println!("{}", tool_msg);
            if let Some(log) = logger.as_mut() {
                let _ = log.logln(&tool_msg);
            }

            match tool.invoke_without_thinking(prompt) {
                Ok((stdout, stderr)) => {
                    if tool.is_rate_limited(&stdout, &stderr) {
                        // Mark this tool as rate limited
                        self.mark_rate_limited(&tool);

                        let rate_limit_msg = format!(
                            "Rate limit detected for {}. Temporarily squelching for 5 minutes.",
                            tool.name()
                        );
                        println!("{}", rate_limit_msg);
                        if let Some(log) = logger.as_mut() {
                            let _ = log.logln(&rate_limit_msg);
                        }

                        if let Some(next_tool) = self.switch_to_next() {
                            let switch_msg =
                                format!("Switching to fallback tool: {}", next_tool.name());
                            println!("{}", switch_msg);
                            if let Some(log) = logger.as_mut() {
                                let _ = log.logln(&switch_msg);
                            }
                            continue;
                        } else {
                            anyhow::bail!("All LLM tools exhausted due to rate limits");
                        }
                    }

                    return Ok((stdout, stderr));
                }
                Err(e) => {
                    let error_msg = format!("Error invoking {}: {}", tool.name(), e);
                    println!("{}", error_msg);
                    if let Some(log) = logger.as_mut() {
                        let _ = log.logln(&error_msg);
                    }

                    if let Some(next_tool) = self.switch_to_next() {
                        let switch_msg =
                            format!("Switching to fallback tool: {}", next_tool.name());
                        println!("{}", switch_msg);
                        if let Some(log) = logger.as_mut() {
                            let _ = log.logln(&switch_msg);
                        }
                        continue;
                    } else {
                        return Err(e);
                    }
                }
            }
        }
    }
}

/// Logger for streaming output to both console and file with buffered writing
struct Logger {
    writer: BufWriter<std::fs::File>,
}

impl Logger {
    fn new(log_path: &str) -> Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
            .with_context(|| format!("Failed to open log file: {}", log_path))?;

        Ok(Self {
            writer: BufWriter::with_capacity(8192, file),
        })
    }

    fn log(&mut self, message: &str) -> Result<()> {
        self.writer.write_all(message.as_bytes())?;
        // Flush periodically to ensure responsive logging
        self.writer.flush()?;
        Ok(())
    }

    fn logln(&mut self, message: &str) -> Result<()> {
        self.writer.write_all(message.as_bytes())?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()?;
        Ok(())
    }
}

#[derive(Parser)]
#[command(name = "afkcode")]
#[command(about = "LLM-powered checklist management and autonomous development loop")]
#[command(version)]
struct Cli {
    /// Path to config file (defaults to afkcode.toml in current directory if it exists)
    #[arg(long, global = true)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum RunMode {
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
enum Commands {
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

        /// Skip the standing orders audit on startup
        #[arg(long)]
        skip_audit: bool,

        /// Override the target file used for the standing orders audit
        #[arg(long)]
        audit_orders_path: Option<PathBuf>,

        /// Disable committing the audit changes automatically
        #[arg(long, hide = true)]
        no_commit_audit: bool,

        /// Comma-separated list of LLM tools to try (codex, claude)
        #[arg(long, default_value = "codex,claude")]
        tools: String,

        /// Log file path for streaming output
        #[arg(long, default_value = "afkcode.log")]
        log_file: String,
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
        #[arg(long, default_value = "codex,claude")]
        tools: String,
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
        #[arg(long, default_value = "codex,claude")]
        tools: String,
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
        #[arg(long, default_value = "codex,claude")]
        tools: String,
    },
}

struct RunConfig {
    checklist: PathBuf,
    checklist_path_str: String,
    controller_prompt: String,
    worker_prompt: String,
    completion_token: String,
    sleep_seconds: u64,
    mode: RunMode,
    skip_audit: bool,
    audit_orders_path: Option<PathBuf>,
    commit_audit: bool,
}

struct WorkerLoopState {
    iteration: usize,
    last_stdout: String,
    saw_stop_token: bool,
    audit_done: bool,
}

enum AuditTarget {
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

fn fill_placeholders(template: &str, checklist: &str, completion_token: &str) -> String {
    template
        .replace("{checklist}", checklist)
        .replace("{completion_token}", completion_token)
        .trim()
        .to_string()
}

fn build_prompt(checklist_path: &str, prompt_template: &str, completion_token: &str) -> String {
    let rendered_body = fill_placeholders(prompt_template, checklist_path, completion_token);
    format!("@{}\n\n{}\n", checklist_path, rendered_body)
}

fn render_core_standing_orders(completion_token: &str) -> String {
    CORE_STANDING_ORDERS_TEMPLATE.replace("{completion_token}", completion_token)
}

fn contains_token(stdout: &str, completion_token: &str) -> bool {
    if completion_token.is_empty() {
        return false;
    }
    stdout
        .to_lowercase()
        .contains(&completion_token.to_lowercase())
}

fn stream_outputs(label: &str, stdout: &str, stderr: &str, logger: &mut Option<Logger>) {
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

fn log_message(logger: &mut Option<Logger>, message: &str) {
    println!("{}", message);
    if let Some(log) = logger.as_mut() {
        let _ = log.logln(message);
    }
    let _ = io::stdout().flush();
}

fn log_warning(logger: &mut Option<Logger>, message: &str) {
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

fn run_worker_loop(
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
        run_standing_orders_audit(config, tool_chain, logger)?;
        state.audit_done = true;
    }

    loop {
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
        sleep_with_log(config.sleep_seconds, logger);
    }

    let _ = state.audit_done;

    Ok(())
}

fn run_controller_worker_loop(
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
        sleep_with_log(config.sleep_seconds, logger);
    }

    Ok(())
}

fn run_standing_orders_audit(
    config: &RunConfig,
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

    let core_orders = render_core_standing_orders(&config.completion_token);

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
        .replace("{completion_token}", &config.completion_token)
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

    maybe_commit_audit_change(&commit_path, config, logger)?;

    Ok(())
}

fn resolve_audit_target(config: &RunConfig) -> Result<AuditTarget> {
    if let Some(path) = &config.audit_orders_path {
        return load_file_audit_target(path);
    }

    let default_agents = PathBuf::from("AGENTS.md");
    if default_agents.exists() {
        return load_file_audit_target(&default_agents);
    }

    let checklist_content = fs::read_to_string(&config.checklist).with_context(|| {
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
    config: &RunConfig,
    logger: &mut Option<Logger>,
) -> Result<()> {
    if !config.commit_audit {
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

fn cmd_run(
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
) -> Result<()> {
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
    };

    let mut tool_chain = LlmToolChain::new(&tools)?;

    match run_config.mode {
        RunMode::Worker => run_worker_loop(&run_config, &mut tool_chain, &mut logger)?,
        RunMode::Controller => {
            run_controller_worker_loop(&run_config, &mut tool_chain, &mut logger)?
        }
    }

    Ok(())
}

fn cmd_init(checklist: PathBuf, title: Option<String>, examples: bool) -> Result<()> {
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

fn cmd_generate(checklist: PathBuf, prompt: String, tools: String) -> Result<()> {
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
    let mut tool_chain = LlmToolChain::new(&tools)?;
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

fn cmd_add(checklist: PathBuf, item: String, sub: bool, section: Option<String>) -> Result<()> {
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

fn cmd_add_batch(checklist: PathBuf, description: String, tools: String) -> Result<()> {
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
    let mut tool_chain = LlmToolChain::new(&tools)?;
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

fn cmd_remove(checklist: PathBuf, pattern: String, yes: bool) -> Result<()> {
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

fn cmd_update(checklist: PathBuf, instruction: String, tools: String) -> Result<()> {
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
    let mut tool_chain = LlmToolChain::new(&tools)?;
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

fn main() -> Result<()> {
    // Handle Ctrl+C gracefully
    ctrlc::set_handler(|| {
        println!("\nInterrupted. Exiting.");
        std::process::exit(0);
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
            skip_audit,
            audit_orders_path,
            no_commit_audit,
            tools,
            log_file,
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
            let merged_skip_audit = config.merge_with_cli(skip_audit, config.skip_audit, false);
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
                "codex,claude".to_string(),
            );
            let merged_log_file = config.merge_with_cli(
                log_file.clone(),
                config.log_file.clone(),
                "afkcode.log".to_string(),
            );

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
        } => {
            let merged_tools = config.merge_with_cli(
                tools.clone(),
                config.tools.clone(),
                "codex,claude".to_string(),
            );
            cmd_generate(checklist, prompt, merged_tools)
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
        } => {
            let merged_tools = config.merge_with_cli(
                tools.clone(),
                config.tools.clone(),
                "codex,claude".to_string(),
            );
            cmd_add_batch(checklist, description, merged_tools)
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
        } => {
            let merged_tools = config.merge_with_cli(
                tools.clone(),
                config.tools.clone(),
                "codex,claude".to_string(),
            );
            cmd_update(checklist, instruction, merged_tools)
        }
    }
}
