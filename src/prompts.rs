pub const DEFAULT_WORKER_PROMPT: &str = r#"@{checklist} Do the thing.

Remember: After completing your task and committing, your turn ends. Do NOT emit {completion_token} unless the checklist now has ZERO incomplete items."#;

/// Worker prompt for multi-checklist mode.
/// Explicitly tells workers NOT to emit the completion token since completion
/// is determined by the orchestrator scanning all AGENTS.md files.
pub const MULTI_CHECKLIST_WORKER_PROMPT: &str = r#"You have been assigned work from the project checklists.

{work_items}

Instructions:
1. Complete the assigned task(s)
2. Update the relevant AGENTS.md file - mark items [x] when done
3. Commit your changes with a descriptive message
4. End your turn

IMPORTANT: Do NOT emit {completion_token} or any completion token.
Completion is determined by the orchestrator scanning all checklists.
Your job is to do work, not judge project completion.
"#;

pub const STOP_CONFIRMATION_PROMPT: &str = r#"I detected that the previous response emitted the stop/completion token "{completion_token}".
Re-open @{checklist}. Confirm that every requirement and task is complete, the code builds cleanly, and all changes are committed.
Emit "{completion_token}" again on a line by itself at the very end ONLY if the loop should end.
If ANYTHING remains, do NOT emit the token. Instead, briefly note what's left (one line), then continue normal work.
"#;

/// Default verifier prompt for auditing implementation completeness.
/// The verifier runs after workers report completion to find missing work.
pub const DEFAULT_VERIFIER_PROMPT: &str = r#"You are verifying implementation completeness for a multi-file project.

First, read the root architecture guide for overall context:
{root_agents_md}

Then scan all component checklists:
{component_checklists}

Your tasks:
1. Audit items marked [x] or [V] - verify they are ACTUALLY complete by reading the implementation code
2. Compare implementations to reference sources (C++, Rust, original code, etc.) for missing features
3. Check test coverage completeness - are there adequate tests for the implemented features?
4. Find TODO/FIXME/stub implementations in actual code files that aren't tracked in checklists

Output: Edit AGENTS.md files ONLY. Either:
- Add new [ ] items for discovered work that needs to be done
- Change [x]/[V] back to [ ] with a sub-bullet explaining what isn't actually complete

Be thorough but precise. Only add items for real gaps, not theoretical improvements.

IMPORTANT: Do NOT emit {completion_token} or any completion token.
Your job is to find missing work, not to declare completion.
Do NOT disturb [ip] items - other workers may be active on those tasks.
"#;
