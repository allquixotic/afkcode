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

//! Stop coordination for parallel LLM subprocesses.
//!
//! Provides thread-safe coordination for signaling and waiting on
//! subprocess completion across multiple parallel LLM instances.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

/// Status of a subprocess.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubprocessStatus {
    /// Subprocess is actively running.
    Running,
    /// Subprocess has finished its current iteration (ready for shutdown).
    FinishingIteration,
    /// Subprocess has completed with a result.
    Completed(SubprocessResult),
}

/// Result of a subprocess completing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubprocessResult {
    /// Completed normally with stop confirmation.
    StopConfirmed,
    /// Shutdown due to external signal (Ctrl+C or coordinator).
    Shutdown,
    /// Error during execution.
    Error(String),
}

/// Coordinates stop signals across all subprocesses.
///
/// When any subprocess confirms completion (stop token twice), it signals
/// all others to finish their current iteration and exit.
pub struct StopCoordinator {
    /// Global stop flag (any subprocess can set this).
    stop_requested: AtomicBool,
    /// Per-subprocess status tracking.
    status: Mutex<HashMap<usize, SubprocessStatus>>,
    /// Number of total subprocesses.
    num_subprocesses: usize,
    /// Condition variable for waiting on status changes.
    condvar: Condvar,
}

impl StopCoordinator {
    /// Create a new coordinator for the given number of subprocesses.
    pub fn new(num_subprocesses: usize) -> Self {
        let mut status = HashMap::new();
        for i in 0..num_subprocesses {
            status.insert(i, SubprocessStatus::Running);
        }
        Self {
            stop_requested: AtomicBool::new(false),
            status: Mutex::new(status),
            num_subprocesses,
            condvar: Condvar::new(),
        }
    }

    /// Signal that a subprocess has confirmed stop.
    ///
    /// This sets the global stop flag and marks the subprocess as completed.
    pub fn signal_stop(&self, subprocess_id: usize) {
        self.stop_requested.store(true, Ordering::SeqCst);
        let mut status = self.status.lock().unwrap();
        status.insert(
            subprocess_id,
            SubprocessStatus::Completed(SubprocessResult::StopConfirmed),
        );
        self.condvar.notify_all();
    }

    /// Check if global stop has been requested.
    pub fn should_stop(&self) -> bool {
        self.stop_requested.load(Ordering::Relaxed)
    }

    /// Mark a subprocess as starting a new iteration (actively running LLM call).
    pub fn mark_iteration_start(&self, subprocess_id: usize) {
        let mut status = self.status.lock().unwrap();
        if let Some(s) = status.get(&subprocess_id) {
            // Only update if not already completed
            if !matches!(s, SubprocessStatus::Completed(_)) {
                status.insert(subprocess_id, SubprocessStatus::Running);
                // No need to notify - we only care about completion states
            }
        }
    }

    /// Mark a subprocess as having finished its current iteration.
    ///
    /// Called after each iteration completes to indicate the subprocess
    /// is at a safe point for shutdown.
    pub fn mark_iteration_complete(&self, subprocess_id: usize) {
        let mut status = self.status.lock().unwrap();
        if let Some(s) = status.get(&subprocess_id) {
            // Only update if currently Running (not already Completed)
            if matches!(s, SubprocessStatus::Running) {
                status.insert(subprocess_id, SubprocessStatus::FinishingIteration);
                self.condvar.notify_all();
            }
        }
    }

    /// Mark a subprocess as fully completed with a result.
    pub fn mark_completed(&self, subprocess_id: usize, result: SubprocessResult) {
        let mut status = self.status.lock().unwrap();
        status.insert(subprocess_id, SubprocessStatus::Completed(result));
        self.condvar.notify_all();
    }

    /// Check if a specific subprocess has completed.
    pub fn is_completed(&self, subprocess_id: usize) -> bool {
        let status = self.status.lock().unwrap();
        matches!(
            status.get(&subprocess_id),
            Some(SubprocessStatus::Completed(_))
        )
    }

    /// Wait for all subprocesses to complete or finish their current iteration.
    ///
    /// Returns `true` if all subprocesses are done, `false` if timeout was reached.
    pub fn wait_for_all_complete(&self, timeout: Duration) -> bool {
        let start = Instant::now();
        let mut status = self.status.lock().unwrap();

        while start.elapsed() < timeout {
            let all_done = status.values().all(|s| {
                matches!(
                    s,
                    SubprocessStatus::Completed(_) | SubprocessStatus::FinishingIteration
                )
            });

            if all_done {
                return true;
            }

            let remaining = timeout.saturating_sub(start.elapsed());
            let result = self.condvar.wait_timeout(status, remaining).unwrap();
            status = result.0;
        }

        false
    }

    /// Get the number of subprocesses.
    pub fn num_subprocesses(&self) -> usize {
        self.num_subprocesses
    }

    /// Get a snapshot of all subprocess statuses.
    pub fn get_statuses(&self) -> HashMap<usize, SubprocessStatus> {
        self.status.lock().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_new_coordinator() {
        let coord = StopCoordinator::new(3);
        assert!(!coord.should_stop());
        assert_eq!(coord.num_subprocesses(), 3);

        let statuses = coord.get_statuses();
        assert_eq!(statuses.len(), 3);
        for i in 0..3 {
            assert_eq!(statuses.get(&i), Some(&SubprocessStatus::Running));
        }
    }

    #[test]
    fn test_signal_stop() {
        let coord = StopCoordinator::new(2);
        assert!(!coord.should_stop());

        coord.signal_stop(0);
        assert!(coord.should_stop());
        assert!(coord.is_completed(0));
        assert!(!coord.is_completed(1));
    }

    #[test]
    fn test_mark_iteration_complete() {
        let coord = StopCoordinator::new(2);

        coord.mark_iteration_complete(0);
        let statuses = coord.get_statuses();
        assert_eq!(
            statuses.get(&0),
            Some(&SubprocessStatus::FinishingIteration)
        );
        assert_eq!(statuses.get(&1), Some(&SubprocessStatus::Running));
    }

    #[test]
    fn test_iteration_lifecycle() {
        let coord = StopCoordinator::new(1);

        // Start in Running state
        assert_eq!(
            coord.get_statuses().get(&0),
            Some(&SubprocessStatus::Running)
        );

        // Complete iteration -> FinishingIteration
        coord.mark_iteration_complete(0);
        assert_eq!(
            coord.get_statuses().get(&0),
            Some(&SubprocessStatus::FinishingIteration)
        );

        // Start new iteration -> back to Running
        coord.mark_iteration_start(0);
        assert_eq!(
            coord.get_statuses().get(&0),
            Some(&SubprocessStatus::Running)
        );

        // Complete again
        coord.mark_iteration_complete(0);
        assert_eq!(
            coord.get_statuses().get(&0),
            Some(&SubprocessStatus::FinishingIteration)
        );
    }

    #[test]
    fn test_mark_completed() {
        let coord = StopCoordinator::new(2);

        coord.mark_completed(0, SubprocessResult::Shutdown);
        assert!(coord.is_completed(0));

        let statuses = coord.get_statuses();
        assert_eq!(
            statuses.get(&0),
            Some(&SubprocessStatus::Completed(SubprocessResult::Shutdown))
        );
    }

    #[test]
    fn test_wait_for_all_complete() {
        let coord = std::sync::Arc::new(StopCoordinator::new(2));

        // Mark both as complete from another thread
        let coord_clone = coord.clone();
        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(10));
            coord_clone.mark_completed(0, SubprocessResult::StopConfirmed);
            coord_clone.mark_completed(1, SubprocessResult::Shutdown);
        });

        let completed = coord.wait_for_all_complete(Duration::from_secs(1));
        assert!(completed);

        handle.join().unwrap();
    }

    #[test]
    fn test_wait_for_all_complete_timeout() {
        let coord = StopCoordinator::new(2);

        // Only mark one as complete
        coord.mark_completed(0, SubprocessResult::StopConfirmed);

        let completed = coord.wait_for_all_complete(Duration::from_millis(50));
        assert!(!completed);
    }
}
