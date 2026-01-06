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
use std::fs::OpenOptions;
use std::io::{BufWriter, Write};

/// Logger for streaming output to both console and file with buffered writing
pub struct Logger {
    writer: BufWriter<std::fs::File>,
}

impl Logger {
    pub fn new(log_path: &str) -> Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
            .with_context(|| format!("Failed to open log file: {}", log_path))?;

        Ok(Self {
            writer: BufWriter::with_capacity(8192, file),
        })
    }

    pub fn log(&mut self, message: &str) -> Result<()> {
        self.writer.write_all(message.as_bytes())?;
        // Flush periodically to ensure responsive logging
        self.writer.flush()?;
        Ok(())
    }

    pub fn logln(&mut self, message: &str) -> Result<()> {
        self.writer.write_all(message.as_bytes())?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()?;
        Ok(())
    }
}
