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
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const DEFAULT_COMPLETION_TOKEN: &str = "__ALL_TASKS_COMPLETE__";

const DEFAULT_CONTROLLER_PROMPT: &str = r#"
You are the controller in an autonomous development loop.
Study the shared checklist in @{checklist}, and reduce the length of it by removing completely finished checklist items.
If and only if all high-level requirements and every checklist item are fully satisfied, output {completion_token} on a line by itself at the very end of your reply; otherwise, do not print that string.
"#;

const DEFAULT_WORKER_PROMPT: &str = "@{checklist} Do the thing.";

const STANDING_ORDERS: &str = r#"# STANDING ORDERS - DO NOT DELETE

1. All additions to this document must be only the minimum amount of information for an LLM to understand the task.
2. When an item is fully complete, remove it from the checklist entirely. If you only partially completed it, add a sub-item with the remaining work and put a ~ in the checkbox instead of an x.
3. When you finish coding, if you discovered new items to work on during your work, add them to this document in the appropriate checklist. Be succinct.
4. Before you finish your work, do a Git commit of files you have modified in this turn.
5. Do not alter or delete any standing orders.
6. Checklist items in this file must never require manual human effort or refer to testing of any kind, only design and coding activities.
7. The command "Do the thing" means: review the remaining to-do items in this file; arbitrarily pick an important item to work on; do that item; update this file, removing 100% complete steps or adding sub-items to partially completed steps, then do a compile of affected projects, making sure they build, fixing errors if not; lastly, do a Git commit of changed files.
8. The command "Fix shit" means: identify to-do items or known issues that are about *broken* code or design, i.e. things that have been left incomplete, code that doesn't compile (errors), or problems that need to be solved, then go solve them, then update this document and do a Git commit.
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
                let wrapped_prompt = format!(
                    "<thinking_mode>disabled</thinking_mode>\n\n{}",
                    prompt
                );
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
        self.rate_limit_timestamps.insert(tool.name().to_string(), Instant::now());
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

    fn invoke_with_fallback(&mut self, prompt: &str, logger: &mut Option<Logger>) -> Result<(String, String)> {
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
                            let switch_msg = format!(
                                "Switching to fallback tool: {}",
                                next_tool.name()
                            );
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
                    let error_msg = format!(
                        "Error invoking {}: {}",
                        tool.name(),
                        e
                    );
                    println!("{}", error_msg);
                    if let Some(log) = logger.as_mut() {
                        let _ = log.logln(&error_msg);
                    }

                    if let Some(next_tool) = self.switch_to_next() {
                        let switch_msg = format!(
                            "Switching to fallback tool: {}",
                            next_tool.name()
                        );
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
    fn invoke_with_fallback_without_thinking(&mut self, prompt: &str, logger: &mut Option<Logger>) -> Result<(String, String)> {
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
                            let switch_msg = format!(
                                "Switching to fallback tool: {}",
                                next_tool.name()
                            );
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
                    let error_msg = format!(
                        "Error invoking {}: {}",
                        tool.name(),
                        e
                    );
                    println!("{}", error_msg);
                    if let Some(log) = logger.as_mut() {
                        let _ = log.logln(&error_msg);
                    }

                    if let Some(next_tool) = self.switch_to_next() {
                        let switch_msg = format!(
                            "Switching to fallback tool: {}",
                            next_tool.name()
                        );
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
        #[arg(long, default_value = DEFAULT_WORKER_PROMPT)]
        worker_prompt: String,

        /// Completion detection token
        #[arg(long, default_value = DEFAULT_COMPLETION_TOKEN)]
        completion_token: String,

        /// Delay between LLM invocations in seconds
        #[arg(long, default_value = "15")]
        sleep_seconds: u64,

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

    let checklist_path = checklist
        .to_str()
        .context("Checklist path contains invalid UTF-8")?;

    let prompts = [("controller", &controller_prompt), ("worker", &worker_prompt)];

    let mut tool_chain = LlmToolChain::new(&tools)?;
    let mut iteration = 0;

    loop {
        let (label, prompt_template) = prompts[iteration % prompts.len()];
        let prompt_text = build_prompt(checklist_path, prompt_template, &completion_token);

        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let timestamp_msg = format!("\n[{}] Running {} prompt...", timestamp, label);
        println!("{}", timestamp_msg);
        if let Some(log) = logger.as_mut() {
            let _ = log.logln(&timestamp_msg);
        }
        let _ = io::stdout().flush();

        let (stdout, stderr) = tool_chain.invoke_with_fallback(&prompt_text, &mut logger)?;
        stream_outputs(label, &stdout, &stderr, &mut logger);

        if label == "controller" && completion_detected(&stdout, &completion_token) {
            // Verify the completion token was intentionally emitted
            if verify_completion_intent(&stdout, &completion_token, &mut tool_chain, &mut logger)? {
                break;
            }
            // If not verified, continue the loop
        }

        iteration += 1;
        let sleep_msg = format!("Sleeping {} seconds before next prompt...", sleep_seconds);
        println!("{}", sleep_msg);
        if let Some(log) = logger.as_mut() {
            let _ = log.logln(&sleep_msg);
        }
        let _ = io::stdout().flush();
        thread::sleep(Duration::from_secs(sleep_seconds));
    }

    Ok(())
}

fn cmd_init(checklist: PathBuf, title: Option<String>, examples: bool) -> Result<()> {
    if checklist.exists() {
        anyhow::bail!(
            "Checklist file already exists: {}",
            checklist.display()
        );
    }

    if let Some(parent) = checklist.parent() {
        fs::create_dir_all(parent)?;
    }

    let title_text = title.unwrap_or_else(|| "Project Checklist".to_string());

    let mut content = format!("# {}\n\n", title_text);
    content.push_str(STANDING_ORDERS);
    content.push_str("\n\n");

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
        anyhow::bail!(
            "Checklist file already exists: {}",
            checklist.display()
        );
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

    full_content.push_str(STANDING_ORDERS);
    full_content.push_str("\n\n");

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

fn cmd_add(
    checklist: PathBuf,
    item: String,
    sub: bool,
    section: Option<String>,
) -> Result<()> {
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
        new_content.push_str(STANDING_ORDERS);
        new_content.push_str("\n\n");

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
                DEFAULT_WORKER_PROMPT.to_string(),
            );
            let merged_completion_token = config.merge_with_cli(
                completion_token.clone(),
                config.completion_token.clone(),
                DEFAULT_COMPLETION_TOKEN.to_string(),
            );
            let merged_sleep_seconds = config.merge_with_cli(
                sleep_seconds,
                config.sleep_seconds,
                15u64,
            );
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
