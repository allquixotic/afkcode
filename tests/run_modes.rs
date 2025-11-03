#![cfg(unix)]

use assert_cmd::Command;
use predicates::str::contains;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

const COMPLETION_TOKEN: &str = "__ALL_TASKS_COMPLETE__";

fn setup_fake_codex(dir: &Path, responses: &[&str]) -> std::io::Result<PathBuf> {
    let bin_dir = dir.join("bin");
    fs::create_dir_all(&bin_dir)?;

    let llm_dir = dir.join("llm");
    fs::create_dir_all(&llm_dir)?;

    for (index, response) in responses.iter().enumerate() {
        let mut file = fs::File::create(llm_dir.join(index.to_string()))?;
        file.write_all(response.as_bytes())?;
    }

    let script_path = bin_dir.join("codex");
    let mut script = fs::File::create(&script_path)?;
    script.write_all(
        br#"#!/bin/bash
set -euo pipefail

DIR="${FAKE_LLM_DIR:?}"
COUNTER_FILE="$DIR/counter"

if [[ ! -f "$COUNTER_FILE" ]]; then
  echo 0 > "$COUNTER_FILE"
fi

COUNTER=$(cat "$COUNTER_FILE")
RESPONSE_FILE="$DIR/$COUNTER"

if [[ ! -f "$RESPONSE_FILE" ]]; then
  echo "fake codex: no response prepared for index $COUNTER" >&2
  exit 1
fi

cat "$RESPONSE_FILE"
COUNTER=$((COUNTER + 1))
echo "$COUNTER" > "$COUNTER_FILE"
"#,
    )?;

    drop(script);
    let mut perms = fs::metadata(&script_path)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script_path, perms)?;

    Ok(llm_dir)
}

fn prepend_path(bin_dir: &Path) -> String {
    let current = std::env::var("PATH").unwrap_or_else(|_| String::new());
    format!("{}:{}", bin_dir.display(), current)
}

fn init_checklist(workdir: &Path, binary: &Path, name: &str) {
    Command::new(binary)
        .arg("init")
        .arg(name)
        .arg("--title")
        .arg("Test Project")
        .current_dir(workdir)
        .assert()
        .success();
}

#[test]
fn worker_stop_token_happy_path() {
    let temp = tempdir().unwrap();
    let workdir = temp.path();

    let responses: Vec<String> = vec![
        format!("{token}\n", token = COMPLETION_TOKEN),
        format!("{token}\n", token = COMPLETION_TOKEN),
    ];
    let response_refs: Vec<&str> = responses.iter().map(|s| s.as_str()).collect();
    let llm_dir = setup_fake_codex(workdir, &response_refs).unwrap();
    let bin_dir = workdir.join("bin");
    let fake_path = prepend_path(&bin_dir);

    let binary = assert_cmd::cargo::cargo_bin!("afkcode");
    init_checklist(workdir, &binary, "checklist.md");

    let log_path = workdir.join("worker.log");

    Command::new(&binary)
        .arg("run")
        .arg("checklist.md")
        .arg("--mode")
        .arg("worker")
        .arg("--skip-audit")
        .arg("--tools")
        .arg("codex")
        .arg("--sleep-seconds")
        .arg("0")
        .arg("--log-file")
        .arg(&log_path)
        .current_dir(workdir)
        .env("PATH", fake_path)
        .env("FAKE_LLM_DIR", &llm_dir)
        .assert()
        .success()
        .stdout(contains("Stop token confirmed; exiting."));

    let counter = fs::read_to_string(llm_dir.join("counter")).unwrap();
    assert_eq!(counter.trim(), "2");

    let log_contents = fs::read_to_string(log_path).unwrap();
    assert!(log_contents.contains("mode=worker iteration=1 turn=normal"));
    assert!(log_contents.contains("mode=worker iteration=2 turn=confirmation"));
}

#[test]
fn worker_stop_token_false_positive_requires_confirmation() {
    let temp = tempdir().unwrap();
    let workdir = temp.path();

    let responses: Vec<String> = vec![
        "We should aim for __ALL_TASKS_COMPLETE__ later.\n".to_string(),
        "Still work remaining.\n".to_string(),
        format!("{token}\n", token = COMPLETION_TOKEN),
        format!("{token}\n", token = COMPLETION_TOKEN),
    ];
    let response_refs: Vec<&str> = responses.iter().map(|s| s.as_str()).collect();
    let llm_dir = setup_fake_codex(workdir, &response_refs).unwrap();
    let bin_dir = workdir.join("bin");
    let fake_path = prepend_path(&bin_dir);

    let binary = assert_cmd::cargo::cargo_bin!("afkcode");
    init_checklist(workdir, &binary, "checklist.md");

    let log_path = workdir.join("false_positive.log");

    Command::new(&binary)
        .arg("run")
        .arg("checklist.md")
        .arg("--mode")
        .arg("worker")
        .arg("--skip-audit")
        .arg("--tools")
        .arg("codex")
        .arg("--sleep-seconds")
        .arg("0")
        .arg("--log-file")
        .arg(&log_path)
        .current_dir(workdir)
        .env("PATH", fake_path)
        .env("FAKE_LLM_DIR", &llm_dir)
        .assert()
        .success()
        .stdout(contains("Stop token confirmed; exiting."));

    let counter = fs::read_to_string(llm_dir.join("counter")).unwrap();
    assert_eq!(counter.trim(), "4");

    let log_contents = fs::read_to_string(log_path).unwrap();
    assert!(log_contents.contains("mode=worker iteration=2 turn=confirmation"));
    assert!(log_contents.contains("mode=worker iteration=2 turn=normal"));
    assert!(log_contents.contains("mode=worker iteration=3 turn=confirmation"));
}

#[test]
fn standing_orders_audit_aligns_and_commits() {
    let temp = tempdir().unwrap();
    let workdir = temp.path();

    Command::new("git")
        .arg("init")
        .current_dir(workdir)
        .assert()
        .success();
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(workdir)
        .assert()
        .success();
    Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(workdir)
        .assert()
        .success();

    fs::write(
        workdir.join("AGENTS.md"),
        "# STANDING ORDERS - DO NOT DELETE\nOld content\n",
    )
    .unwrap();
    fs::write(workdir.join("checklist.md"), "# Test Checklist\n").unwrap();

    let core_orders = format!(
        "# STANDING ORDERS - DO NOT DELETE\n\n1. Minimal Information: Checklist items contain only the minimum needed for an LLM to act.\n2. Completion Handling: Delete fully complete items. For partials, change `[ ]` to `[~]` and add sub-items for the remaining work.\n3. Discovery: Add newly discovered work as new (sub-)items, succinctly.\n4. Git Commit: Before finishing a work turn, run `git add` and `git commit` with a descriptive message summarizing changes.\n5. Immutability: The \"STANDING ORDERS\" section is immutable except during the one-time alignment step run by afkcode.\n6. No Manual Work: Do not require or mention manual human steps or manual testing; prefer automated tests and processes.\n7. \"Do the thing\": Review checklist, pick an important incomplete item, implement fully or partially, update checklist, build, fix errors, commit.\n8. \"Fix shit\": Identify broken code/design or incomplete implementations, fix, update checklist, commit.\n9. Stop Token Etiquette (Worker Mode): Emit `{token}` on a line by itself at the very end ONLY when all requirements are met, no `[ ]` or `[~]` remain, the code builds cleanly, and all changes are committed.\n\n# Project Standing Orders\n\n",
        token = COMPLETION_TOKEN
    );

    let responses: Vec<String> = vec![
        core_orders.clone(),
        format!("{token}\n", token = COMPLETION_TOKEN),
        format!("{token}\n", token = COMPLETION_TOKEN),
    ];
    let response_refs: Vec<&str> = responses.iter().map(|s| s.as_str()).collect();

    let llm_dir = setup_fake_codex(workdir, &response_refs).unwrap();
    let bin_dir = workdir.join("bin");
    let fake_path = prepend_path(&bin_dir);

    let binary = assert_cmd::cargo::cargo_bin!("afkcode");
    let log_path = workdir.join("audit.log");

    Command::new(&binary)
        .arg("run")
        .arg("checklist.md")
        .arg("--mode")
        .arg("worker")
        .arg("--tools")
        .arg("codex")
        .arg("--sleep-seconds")
        .arg("0")
        .arg("--log-file")
        .arg(&log_path)
        .current_dir(workdir)
        .env("PATH", fake_path)
        .env("FAKE_LLM_DIR", &llm_dir)
        .assert()
        .success();

    let agents = fs::read_to_string(workdir.join("AGENTS.md")).unwrap();
    assert!(agents.starts_with(&core_orders));

    let commit_subject = Command::new("git")
        .args(["log", "-1", "--pretty=%s"])
        .current_dir(workdir)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8(commit_subject.stdout).unwrap().trim(),
        "afkcode: align Standing Orders (core v1)"
    );
}

#[test]
fn controller_mode_backwards_compatible() {
    let temp = tempdir().unwrap();
    let workdir = temp.path();

    let responses: Vec<String> = vec![
        format!("{token}\n", token = COMPLETION_TOKEN),
        format!("{token}\n", token = COMPLETION_TOKEN),
    ];
    let response_refs: Vec<&str> = responses.iter().map(|s| s.as_str()).collect();
    let llm_dir = setup_fake_codex(workdir, &response_refs).unwrap();
    let bin_dir = workdir.join("bin");
    let fake_path = prepend_path(&bin_dir);

    let binary = assert_cmd::cargo::cargo_bin!("afkcode");
    init_checklist(workdir, &binary, "checklist.md");

    let log_path = workdir.join("controller.log");

    Command::new(&binary)
        .arg("run")
        .arg("checklist.md")
        .arg("--mode")
        .arg("controller")
        .arg("--skip-audit")
        .arg("--tools")
        .arg("codex")
        .arg("--sleep-seconds")
        .arg("0")
        .arg("--log-file")
        .arg(&log_path)
        .current_dir(workdir)
        .env("PATH", fake_path)
        .env("FAKE_LLM_DIR", &llm_dir)
        .assert()
        .success();

    let counter = fs::read_to_string(llm_dir.join("counter")).unwrap();
    assert_eq!(counter.trim(), "2");

    let log_contents = fs::read_to_string(log_path).unwrap();
    assert!(log_contents.contains("Running controller prompt"));
}
