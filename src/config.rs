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
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Configuration file structure
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Config {
    /// LLM tools to use (comma-separated: gemini, codex, claude)
    pub tools: Option<String>,

    /// Sleep duration between LLM calls (in seconds)
    pub sleep_seconds: Option<u64>,

    /// Controller prompt template
    pub controller_prompt: Option<String>,

    /// Worker prompt template
    pub worker_prompt: Option<String>,

    /// Completion detection token
    pub completion_token: Option<String>,

    /// Log file path for streaming output
    pub log_file: Option<String>,

    /// Run mode (worker or controller)
    pub mode: Option<String>,

    /// Optional path override for standing orders audit
    pub orders_path: Option<String>,

    /// Skip the standing orders audit on startup
    pub skip_audit: Option<bool>,

    /// Commit audit changes automatically
    pub commit_audit: Option<bool>,

    /// Per-tool model specifications (e.g., gemini_model, claude_model, codex_model)
    pub gemini_model: Option<String>,
    pub claude_model: Option<String>,
    pub codex_model: Option<String>,
    
    /// Warp Agent API configuration
    pub warp_api_key: Option<String>,
    pub warp_model: Option<String>,
}

impl Config {
    /// Load config from a file, or return default if file doesn't exist
    pub fn load(path: &PathBuf) -> Result<Self> {
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
    pub fn merge_with_cli<T>(&self, cli_value: T, config_value: Option<T>, default_value: T) -> T
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
