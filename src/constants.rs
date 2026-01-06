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

pub const DEFAULT_COMPLETION_TOKEN: &str = "__ALL_TASKS_COMPLETE__";

pub const DEFAULT_CONTROLLER_PROMPT: &str = r#"
You are the controller in an autonomous development loop.
Study the shared checklist in @{checklist}, and reduce the length of it by removing completely finished checklist items.
If and only if all high-level requirements and every checklist item are fully satisfied, output {completion_token} on a line by itself at the very end of your reply; otherwise, do not print that string.
"#;

pub const CORE_STANDING_ORDERS_VERSION: &str = "1";

pub const CORE_STANDING_ORDERS_TEMPLATE: &str = r#"# STANDING ORDERS - DO NOT DELETE

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

pub const WARP_AGENT_API_BASE: &str = "https://app.warp.dev/api/v1";

pub const STANDING_ORDERS_AUDIT_PROMPT_TEMPLATE: &str = r#"You are aligning the repository's Standing Orders for afkcode's worker-only mode.

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

pub fn render_core_standing_orders(completion_token: &str) -> String {
    CORE_STANDING_ORDERS_TEMPLATE.replace("{completion_token}", completion_token)
}
