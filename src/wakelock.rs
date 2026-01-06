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

//! System wake lock to prevent sleep during LLM execution.
//!
//! Prevents the system from entering S3 sleep state while LLM subprocesses
//! are running. Uses OS-native facilities that are automatically released
//! when the process exits (even if forcibly killed with SIGKILL).
//!
//! Platform implementations:
//! - **macOS**: IOPMAssertionCreateWithName (process-bound assertion)
//! - **Windows**: SetThreadExecutionState (thread-bound, cleared on exit)
//! - **Linux**: D-Bus inhibitor via org.freedesktop.login1.Manager.Inhibit
//!
//! All platforms release the wake lock automatically when the process exits,
//! ensuring the system can sleep even if afkcode crashes or is killed.

use anyhow::Result;

/// Guard that keeps the system awake while held.
///
/// The wake lock is automatically released when this guard is dropped.
/// This uses RAII semantics - simply let the guard go out of scope to
/// release the lock.
///
/// # Example
///
/// ```no_run
/// use afkcode::wakelock::WakeLock;
///
/// fn run_llm_work() -> anyhow::Result<()> {
///     // Acquire wake lock - system won't sleep while this is held
///     let _wake_guard = WakeLock::acquire()?;
///
///     // Do LLM work here...
///
///     // Wake lock automatically released when _wake_guard is dropped
///     Ok(())
/// }
/// ```
pub struct WakeLock {
    #[allow(dead_code)]
    inner: keepawake::KeepAwake,
}

impl WakeLock {
    /// Acquire a wake lock to prevent system sleep.
    ///
    /// This prevents idle sleep (system sleeping due to inactivity).
    /// The lock is automatically released when the returned guard is dropped
    /// or when the process exits (including SIGKILL).
    ///
    /// # Errors
    ///
    /// Returns an error if the wake lock cannot be acquired (e.g., missing
    /// D-Bus on Linux, or insufficient permissions).
    pub fn acquire() -> Result<Self> {
        let inner = keepawake::Builder::default()
            .idle(true)
            .reason("Running LLM subprocesses")
            .app_name("afkcode")
            .app_reverse_domain("com.github.allquixotic.afkcode")
            .create()
            .map_err(|e| anyhow::anyhow!("Failed to acquire wake lock: {}", e))?;

        Ok(Self { inner })
    }

    /// Try to acquire a wake lock, returning None on failure.
    ///
    /// This is useful when you want to proceed even if the wake lock
    /// cannot be acquired (e.g., running in a container without D-Bus).
    pub fn try_acquire() -> Option<Self> {
        Self::acquire().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_acquire_wake_lock() {
        // This test may fail in CI environments without proper D-Bus/power management
        // so we just test that try_acquire doesn't panic
        let _lock = WakeLock::try_acquire();
        // Lock is released when dropped
    }

    #[test]
    fn test_wake_lock_drop() {
        // Verify RAII works - acquire and immediately drop
        if let Some(lock) = WakeLock::try_acquire() {
            drop(lock);
            // Should be able to acquire again
            let _lock2 = WakeLock::try_acquire();
        }
    }
}
