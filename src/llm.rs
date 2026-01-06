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

use anyhow::{anyhow, Context, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

use crate::constants::WARP_AGENT_API_BASE;
use crate::logger::Logger;

/// Warp Agent API request/response types
#[derive(Debug, Serialize)]
pub struct RunAgentRequest {
    prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    config: Option<AmbientAgentConfig>,
}

#[derive(Debug, Serialize)]
pub struct AmbientAgentConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    model_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RunAgentResponse {
    task_id: String,
}

#[derive(Debug, Deserialize)]
pub struct TaskResponse {
    state: String,
    #[serde(default)]
    session_link: Option<String>,
}

/// LLM tool kind
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmToolKind {
    Gemini,
    Codex,
    Claude,
    WarpAgent,
}

/// LLM tool configuration and invocation logic
#[derive(Debug, Clone)]
pub struct LlmTool {
    kind: LlmToolKind,
    model: Option<String>,
    api_key: Option<String>,  // For HTTP-based tools like Warp Agent
}

impl LlmTool {
    pub fn from_name(name: &str) -> Result<Self> {
        let kind = match name.to_lowercase().as_str() {
            "gemini" => LlmToolKind::Gemini,
            "codex" => LlmToolKind::Codex,
            "claude" => LlmToolKind::Claude,
            "warp" | "warp-agent" => LlmToolKind::WarpAgent,
            _ => anyhow::bail!("Unsupported LLM tool: {}. Supported: gemini, codex, claude, warp", name),
        };
        Ok(Self { kind, model: None, api_key: None })
    }

    pub fn with_model(mut self, model: Option<String>) -> Self {
        self.model = model;
        self
    }
    
    pub fn with_api_key(mut self, api_key: Option<String>) -> Self {
        self.api_key = api_key;
        self
    }

    pub fn name(&self) -> &'static str {
        match self.kind {
            LlmToolKind::Gemini => "gemini",
            LlmToolKind::Codex => "codex",
            LlmToolKind::Claude => "claude",
            LlmToolKind::WarpAgent => "warp",
        }
    }

    fn command(&self) -> &'static str {
        match self.kind {
            LlmToolKind::Gemini => "gemini",
            LlmToolKind::Codex => "codex",
            LlmToolKind::Claude => "claude",
            LlmToolKind::WarpAgent => "warp",  // Not actually used for HTTP-based tool
        }
    }

    fn args(&self) -> Vec<String> {
        let mut args = match self.kind {
            LlmToolKind::Gemini => vec!["--yolo".to_string()],
            LlmToolKind::Codex => vec!["exec".to_string()],
            LlmToolKind::Claude => vec![
                "--print".to_string(),
                "--dangerously-skip-permissions".to_string(),
            ],
            LlmToolKind::WarpAgent => vec![],  // HTTP-based, no CLI args
        };

        // Add model argument if specified
        if let Some(ref model) = self.model {
            match self.kind {
                LlmToolKind::Gemini => {
                    args.push("-m".to_string());
                    args.push(model.clone());
                }
                LlmToolKind::Codex => {
                    args.push("-m".to_string());
                    args.push(model.clone());
                }
                LlmToolKind::Claude => {
                    args.push("--model".to_string());
                    args.push(model.clone());
                }
                LlmToolKind::WarpAgent => {
                    // Model handled via HTTP API, not CLI args
                }
            }
        }

        args
    }

    fn rate_limit_patterns(&self) -> Vec<&'static str> {
        match self.kind {
            LlmToolKind::Gemini => vec![
                "rate limit",
                "quota exceeded",
                "429",
                "too many requests",
                "resource exhausted",
            ],
            LlmToolKind::Codex => vec![
                "rate limit reached",
                "rate_limit_error",
                "429",
                "too many requests",
            ],
            LlmToolKind::Claude => vec![
                "usage limit reached",
                "rate limit reached",
                "rate_limit_error",
                "429",
                "limit will reset",
            ],
            LlmToolKind::WarpAgent => vec![
                "rate limit",
                "429",
                "too many requests",
                "quota exceeded",
            ],
        }
    }

    pub fn is_rate_limited(&self, stdout: &str, stderr: &str) -> bool {
        let combined = format!("{}{}", stdout, stderr).to_lowercase();
        self.rate_limit_patterns()
            .iter()
            .any(|pattern| combined.contains(pattern))
    }

    /// Invoke Warp Agent via HTTP API
    fn invoke_warp_agent(&self, prompt: &str) -> Result<(String, String)> {
        let api_key = self.api_key.as_ref()
            .ok_or_else(|| anyhow!("Warp Agent API key not configured. Set WARP_API_KEY environment variable or warp_api_key in config"))?;
        
        let client = Client::builder()
            .timeout(Duration::from_secs(300))
            .build()?;
        
        // Create agent task
        let request = RunAgentRequest {
            prompt: prompt.to_string(),
            title: Some("afkcode task".to_string()),
            config: Some(AmbientAgentConfig {
                model_id: self.model.clone(),
            }),
        };
        
        let create_url = format!("{}/agent/run", WARP_AGENT_API_BASE);
        let response = client
            .post(&create_url)
            .header("Authorization", format!("Bearer {}", api_key))
            .json(&request)
            .send()
            .context("Failed to create Warp Agent task")?;
        
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            anyhow::bail!("Warp Agent API returned error {}: {}", status, body);
        }
        
        let run_response: RunAgentResponse = response.json()
            .context("Failed to parse Warp Agent response")?;
        let task_id = run_response.task_id;
        
        // Poll for completion
        let task_url = format!("{}/agent/tasks/{}", WARP_AGENT_API_BASE, task_id);
        let mut attempts = 0;
        let max_attempts = 180;  // 3 minutes with 1-second polls
        
        loop {
            thread::sleep(Duration::from_secs(1));
            attempts += 1;
            
            if attempts > max_attempts {
                anyhow::bail!("Warp Agent task timed out after {} seconds", max_attempts);
            }
            
            let task_response = client
                .get(&task_url)
                .header("Authorization", format!("Bearer {}", api_key))
                .send()
                .context("Failed to get Warp Agent task status")?;
            
            if !task_response.status().is_success() {
                let status = task_response.status();
                let body = task_response.text().unwrap_or_default();
                return Ok((String::new(), format!("API error {}: {}", status, body)));
            }
            
            let task: TaskResponse = task_response.json()
                .context("Failed to parse task status")?;
            
            match task.state.as_str() {
                "SUCCEEDED" => {
                    // For now, return the session link as the output
                    // In the future, we might want to fetch the actual agent output
                    let output = task.session_link.unwrap_or_else(|| "Task completed".to_string());
                    return Ok((output, String::new()));
                }
                "FAILED" => {
                    return Ok((String::new(), "Warp Agent task failed".to_string()));
                }
                "QUEUED" | "INPROGRESS" => {
                    // Continue polling
                    continue;
                }
                _ => {
                    // Unknown state, continue polling
                    continue;
                }
            }
        }
    }
    
    pub fn invoke(&self, prompt: &str) -> Result<(String, String)> {
        // Handle HTTP-based tools differently
        if self.kind == LlmToolKind::WarpAgent {
            return self.invoke_warp_agent(prompt);
        }
        
        let mut cmd = Command::new(self.command());
        cmd.args(self.args());

        // Gemini takes the prompt as a positional argument, others use stdin
        if self.kind == LlmToolKind::Gemini {
            cmd.arg(prompt);
        }

        // Spawn in a new process group so it won't receive terminal SIGINT
        // This allows graceful Ctrl+C handling - the parent catches SIGINT and
        // waits for the child to complete instead of both being killed
        #[cfg(unix)]
        let child_result = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn();
        #[cfg(not(unix))]
        let child_result = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();

        let mut child = child_result.with_context(|| {
            format!(
                "Failed to spawn {} process. Is {} CLI installed?",
                self.name(),
                self.name()
            )
        })?;

        // For non-Gemini tools, write prompt to stdin
        if self.kind != LlmToolKind::Gemini {
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(prompt.as_bytes())?;
            }
        }

        let output = child.wait_with_output()?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        Ok((stdout, stderr))
    }

    /// Invoke the LLM with thinking disabled for simple verification tasks
    pub fn invoke_without_thinking(&self, prompt: &str) -> Result<(String, String)> {
        match self.kind {
            LlmToolKind::Gemini => {
                // Gemini doesn't have a thinking mode toggle, just invoke normally
                self.invoke(prompt)
            }
            LlmToolKind::Claude => {
                // For Claude Code, wrap the prompt with thinking_mode disabled
                let wrapped_prompt =
                    format!("<thinking_mode>disabled</thinking_mode>\n\n{}", prompt);
                self.invoke(&wrapped_prompt)
            }
            LlmToolKind::WarpAgent => {
                // Warp Agent API doesn't have a direct thinking toggle
                // Just invoke normally
                self.invoke(prompt)
            }
            LlmToolKind::Codex => {
                // For Codex CLI, use minimal reasoning effort
                let mut args = self.args();
                args.push("-c".to_string());
                args.push("model_reasoning_effort=\"minimal\"".to_string());

                let mut cmd = Command::new(self.command());
                cmd.args(args)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());

                // Spawn in a new process group so it won't receive terminal SIGINT
                #[cfg(unix)]
                let child_result = cmd.process_group(0).spawn();
                #[cfg(not(unix))]
                let child_result = cmd.spawn();

                let mut child = child_result.with_context(|| {
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

/// Per-tool model configuration
#[derive(Debug, Clone, Default)]
pub struct ModelConfig {
    pub gemini_model: Option<String>,
    pub claude_model: Option<String>,
    pub codex_model: Option<String>,
    pub warp_model: Option<String>,
    pub warp_api_key: Option<String>,
}

impl ModelConfig {
    pub fn get_model_for_tool(&self, kind: LlmToolKind) -> Option<String> {
        match kind {
            LlmToolKind::Gemini => self.gemini_model.clone(),
            LlmToolKind::Claude => self.claude_model.clone(),
            LlmToolKind::Codex => self.codex_model.clone(),
            LlmToolKind::WarpAgent => self.warp_model.clone(),
        }
    }
    
    pub fn get_api_key_for_tool(&self, kind: LlmToolKind) -> Option<String> {
        match kind {
            LlmToolKind::WarpAgent => self.warp_api_key.clone(),
            _ => None,
        }
    }
}

/// Manages multiple LLM tools with automatic fallback
pub struct LlmToolChain {
    tools: Vec<LlmTool>,
    current_index: usize,
    rate_limit_timestamps: HashMap<String, Instant>,
    rate_limit_timeout: Duration,
}

impl LlmToolChain {
    #[allow(dead_code)]
    pub fn new(tool_names: &str) -> Result<Self> {
        Self::with_models(tool_names, &ModelConfig::default())
    }

    pub fn with_models(tool_names: &str, model_config: &ModelConfig) -> Result<Self> {
        let tools: Result<Vec<_>> = tool_names
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|name| {
                let tool = LlmTool::from_name(name)?;
                let model = model_config.get_model_for_tool(tool.kind);
                let api_key = model_config.get_api_key_for_tool(tool.kind);
                Ok(tool.with_model(model).with_api_key(api_key))
            })
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

    pub fn invoke_with_fallback(
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
    pub fn invoke_with_fallback_without_thinking(
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
