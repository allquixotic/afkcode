# afkcode - Technical Documentation for LLM Agents

> **Note**: This document is designed for LLM consumption. For human-readable documentation, see README.md.

## Purpose

`afkcode` is a command-line tool for managing project checklists and running autonomous development loops. When you (an LLM) are invoked through this tool, you will receive checklist context and instructions for making incremental progress on software projects.

## Architecture

### Core Components

1. **Controller**: Reviews checklist, summarizes progress, assigns next task
2. **Worker**: Implements the assigned task, updates checklist, commits changes
3. **Standing Orders**: Immutable behavioral invariants that govern your actions
4. **Checklist**: Markdown file tracking requirements, tasks, and progress

### File Format

Checklists are Markdown files with this structure:

```markdown
# Project Title

# STANDING ORDERS - DO NOT DELETE
[9 numbered invariant rules]

# High-Level Requirements
- [ ] Requirement 1
- [ ] Requirement 2

# Section Name
- [ ] Task 1
    - [ ] Subtask 1.1
    - [ ] Subtask 1.2
- [~] Task 2 (partially complete)
    - [ ] Remaining work

# Notes
Additional context
```

### Checkbox States

- `[ ]` - Incomplete task
- `[x]` - Complete task (should be deleted per Standing Order #2)
- `[~]` - Partially complete task (add sub-items for remaining work)
- `[ip]` - In-progress task (claimed by a worker via gimme mode)
- `[ip:XXXX]` - In-progress task with checkout ID (for tracking which worker claimed it)
- `[V]` - Verified complete (used by verifier mode)
- `[BLOCKED]` - Task is blocked awaiting resolution

## Standing Orders (Behavioral Invariants)

These rules are **immutable** and govern all LLM behavior when working with checklists:

> **CRITICAL**: `__ALL_TASKS_COMPLETE__` means the ENTIRE PROJECT is finished. Do NOT emit this token after completing a single task. Only emit it after verifying the checklist contains ZERO `[ ]` or `[~]` items.

### 1. Minimal Information
Keep each checklist item to the minimum details an LLM needs to act. Avoid verbose descriptions.

**Good**: `[ ] Implement JWT authentication with RS256 signing`
**Bad**: `[ ] We should probably think about adding some kind of authentication system, maybe using JWT tokens or something similar, with proper signing to ensure security`

### 2. Completion Handling
- **Fully complete items**: Delete entirely from checklist
- **Partially complete items**: Change `[ ]` to `[~]` and add sub-items for remaining work

**Example:**
```markdown
Before:
- [ ] Implement user service

After (partial completion):
- [~] Implement user service
    - [ ] Add password reset endpoint
    - [ ] Add email verification
```

### 3. Discovery Documentation
When implementing a task, if you discover new work items, add them to the checklist in the appropriate section. Be succinct.

### 4. Git Commit Requirement
Before finishing your work session, execute `git add` and `git commit` with a descriptive message summarizing changes made.

### 5. Standing Orders Immutability
The "STANDING ORDERS" section is immutable except during the one-time alignment step run by afkcode. Do not edit it in normal turns.

### 6. No Manual Work References
Checklist items must **never** require manual human effort or mention testing. Focus only on:
- Design activities
- Coding activities
- Automated processes

Prefer automated testing workflows over manual spot checks.

**Prohibited**: `[ ] Manually test the login flow in a browser`
**Allowed**: `[ ] Implement automated integration tests for login flow`

### 7. "Do the thing" Command Semantics

When you receive the prompt "@{checklist} Do the thing.", execute this workflow:

1. **Review**: Read all remaining to-do items in the checklist
2. **Select**: Arbitrarily pick an important incomplete item
3. **Implement**: Complete that item fully or partially
4. **Update Checklist**:
   - Remove 100% complete items
   - Add sub-items to partially complete items (change `[ ]` to `[~]`)
   - Add newly discovered items
5. **Compile**: Run build commands for affected projects, fix any errors
6. **Commit**: Execute `git add` and `git commit` with descriptive message
7. **End Turn**: Your turn is complete. Do NOT emit `__ALL_TASKS_COMPLETE__` unless you have verified the checklist now contains ZERO incomplete items.

### 8. "Fix shit" Command Semantics

When you receive this instruction, execute this workflow:

1. **Identify**: Find to-do items or known issues related to:
   - Broken code or design
   - Incomplete implementations
   - Compilation errors
   - Problems requiring resolution
2. **Fix**: Solve the identified problems
3. **Update**: Modify checklist to reflect fixes
4. **Commit**: Execute `git add` and `git commit`
5. **End Turn**: Your turn is complete. Do NOT emit `__ALL_TASKS_COMPLETE__` unless the checklist is now empty.

### 9. Stop Token Etiquette (Worker Mode)

**DEFAULT: Do NOT emit `__ALL_TASKS_COMPLETE__`.** The vast majority of your turns will NOT end with this token.

**Single-checklist mode**: You may emit the completion token ONLY when ALL of these conditions are true:
- You have re-read the ENTIRE checklist file
- There are ZERO `[ ]` items remaining
- There are ZERO `[~]` items remaining
- The code builds cleanly
- All changes are committed

**Multi-checklist mode** (using `--checklist-dir`): **NEVER emit the token.** Completion is determined externally by the orchestrator scanning all AGENTS.md files. Your job is to do work, not judge project-wide completion.

**Completing your assigned task does NOT mean emitting the token.** Finishing one task simply means updating the checklist and committing—then your turn ends normally WITHOUT the token.

If unsure which mode you're in, do NOT emit the token.

> Checklist hygiene (short bullets, deleting completed items, using sub-items for partial work) is enforced by these Standing Orders; afkcode does not rewrite the checklist during worker turns.

## Invocation Modes

### Multi-Checklist Mode

When afkcode is run with `--checklist-dir <PATH>`, it enters multi-checklist mode which supports hierarchical project structures with multiple AGENTS.md files.

**Structure Example**:
```
project/
├── AGENTS.md              # Root architecture guide
├── component-a/
│   └── AGENTS.md          # Component A checklist
├── component-b/
│   └── AGENTS.md          # Component B checklist
└── shared/
    └── AGENTS.md          # Shared utilities checklist
```

**Key Differences from Single-Checklist Mode**:
1. Workers **NEVER** emit the completion token (completion is determined by scanning all files)
2. afkcode uses deterministic scanning to detect when all checklists have zero incomplete items
3. Tasks are claimed across all AGENTS.md files using `[ip:XXXX]` checkout markers

**Gimme Mode**: In multi-checklist mode, workers receive pre-assigned tasks with checkout IDs. The `[ip:XXXX]` marker prevents multiple workers from claiming the same task.

### Verifier Mode

When using multi-checklist mode with `--verify`, afkcode runs a verification phase after workers complete:

```bash
afkcode run --checklist-dir project/ --verify
```

The verifier LLM:
1. Audits items marked `[x]` or `[V]` to verify actual completeness
2. Compares implementations to reference sources for missing features
3. Checks test coverage completeness
4. Finds TODO/FIXME/stub implementations in code that aren't tracked in checklists

If the verifier finds missing work, it adds new `[ ]` items to the appropriate AGENTS.md files.

### Spiral Mode

Spiral mode (`--spiral`) enables automatic restart of workers when the verifier finds new work:

```bash
afkcode run --checklist-dir project/ --verify --spiral --max-spirals 5
```

**Flow**:
```
Workers → Scan (complete?) → Verifier → Found work? → Workers → ...
                    ↓                        ↓
              No incomplete             No new work
                    ↓                        ↓
                 [EXIT]                   [EXIT]
```

The loop continues until:
- No incomplete items are found (all `[ ]`, `[~]`, `[ip]` cleared)
- Verifier finds no new work
- Max spirals reached (default: 5)

### Controller Mode

**Prompt Pattern**:
```
You are the controller in an autonomous development loop.
Study the shared checklist in @{checklist}, summarize current progress, and update it as needed.
Assign the next actionable task to the worker so momentum continues.
If—and only if—all high-level requirements and every checklist item are fully satisfied,
output {completion_token} on a line by itself at the very end of your reply;
otherwise, do not print that string.
```

**Your Responsibilities**:
1. Read entire checklist (via `@{checklist}` reference)
2. Summarize current state (what's done, what's pending)
3. Identify highest-priority incomplete task
4. Update checklist if needed (reorganize, clarify, add context)
5. Provide clear assignment to worker
6. **Only when everything is complete**: Output `__ALL_TASKS_COMPLETE__` (or custom token)

**Critical**: Do NOT output the completion token unless **all** requirements and tasks are satisfied. Premature completion aborts the development loop.

### Worker Mode

**Prompt Pattern**:
```
@{checklist} Do the thing.
```

**Your Responsibilities**:
1. Execute "Do the thing" workflow (see Standing Order #7)
2. Make actual code changes
3. Update checklist to reflect progress
4. Commit changes with meaningful message
5. Report completion and any issues encountered

## Command Context

When invoked through specific afkcode commands, understand the context:

### `run` Command

**Single-checklist mode** (default):
```bash
afkcode run CHECKLIST.md
```
Worker mode is the default: you receive the worker prompt repeatedly, with a one-time standing-orders alignment turn at the start (unless the user skips it). If `--mode controller` is supplied, controller and worker turns alternate as before. Maintain continuity across iterations by reading the updated checklist each time.

**Multi-checklist mode**:
```bash
afkcode run --checklist-dir project/ [--verify] [--spiral] [--max-spirals 5]
```
Scans all AGENTS.md files under the given directory. Workers receive pre-assigned tasks and never emit the completion token. Completion is determined by the scanner finding zero incomplete items.

**CLI Options**:
- `--num-instances N`: Run N parallel worker instances (default: 1)
- `--warmup-delay S`: Seconds between launching instances (default: 30)
- `--verify`: Enable verifier phase after workers complete
- `--verifier-prompt PATH`: Custom verifier prompt file
- `--spiral`: Auto-restart workers if verifier finds work
- `--max-spirals N`: Maximum spiral iterations (default: 5)
- `--no-gimme`: Disable gimme mode (task checkout)

### `generate` Command
You are being asked to create a complete project checklist from a high-level description. Output ONLY:
- H1 title
- High-level requirements section
- Task breakdown sections
- Any relevant notes

Do NOT include standing orders in your output (they are prepended automatically).

### `add-batch` Command
You are expanding a high-level description into specific checklist items. Output ONLY checkbox items in this format:
```markdown
- [ ] Item 1
- [ ] Item 2
    - [ ] Sub-item 2.1
```

No explanations, headers, or surrounding text.

### `update` Command
You are modifying an existing checklist according to specific instructions. Output the COMPLETE updated checklist, preserving all standing orders and structure.

## Best Practices

### Task Granularity
Break large features into small, incremental tasks:

**Too Large**:
```markdown
- [ ] Implement user authentication system
```

**Appropriate**:
```markdown
- [ ] Add User model with password hash field
- [ ] Implement bcrypt password hashing service
- [ ] Create POST /auth/login endpoint
- [ ] Add JWT token generation
- [ ] Implement authentication middleware
```

### Commit Messages
Write clear, descriptive commit messages:

**Good**:
- `Add JWT authentication with RS256 signing`
- `Fix null pointer in user lookup service`
- `Refactor database connection pooling for efficiency`

**Bad**:
- `update`
- `changes`
- `wip`

### Section Organization
Use clear section headers:
- `# High-Level Requirements` - What needs to be built
- `# Architecture` - Design decisions and structure
- `# Tasks` - Concrete implementation items
- `# Known Issues` - Bugs and problems to fix
- `# Notes` - Additional context

### Sub-Item Indentation
Use exactly 4 spaces for sub-items:
```markdown
- [ ] Parent task
    - [ ] Sub-task (4 spaces)
        - [ ] Sub-sub-task (8 spaces)
```

## Error Handling

### Rate Limits
If you detect rate limit responses, the loop will automatically terminate. Do not attempt to continue.

### Compilation Errors
If code fails to compile after your changes, you MUST fix it before finishing. Do not leave the codebase in a broken state.

### Missing Context
If the checklist lacks sufficient context to complete a task, add a note requesting clarification as a sub-item, mark the parent as `[~]`, and move to a different task.

## Advanced Patterns

### Progressive Enhancement
Start with minimal viable implementation, then enhance:

```markdown
- [~] Implement user service
    - [x] Basic CRUD operations
    - [x] Password hashing
    - [ ] Email verification
    - [ ] Password reset flow
    - [ ] Rate limiting
```

### Dependency Tracking
Use sub-items to show dependencies:

```markdown
- [ ] Deploy API to production
    - [ ] Set up database migrations
    - [ ] Configure environment variables
    - [ ] Set up load balancer
    - [ ] Configure TLS certificates
```

### Iterative Refinement
Controller can refine tasks discovered to be more complex:

```markdown
Before:
- [ ] Add authentication

After (controller refinement):
- [ ] Add authentication
    - [ ] Implement password hashing (bcrypt)
    - [ ] Add JWT token generation (RS256)
    - [ ] Create /auth/login endpoint
    - [ ] Create /auth/logout endpoint
    - [ ] Add authentication middleware
    - [ ] Update API routes with auth checks
```

## Integration Points

### File References
When you see `@path/to/file.md`, this is a reference to a file on disk. The LLM CLI you're running through (Codex, Claude Code, etc.) will expand this reference automatically.

### Git Operations
You have access to git commands. Use them for:
- `git add .` - Stage all changes
- `git commit -m "message"` - Commit with message
- `git status` - Check repository state
- `git diff` - Review changes

Do NOT use:
- `git push` (unless explicitly instructed)
- `git reset --hard` (destructive)
- `git rebase -i` (interactive, not supported)

### Build Commands
Common patterns:
- Rust: `cargo build`, `cargo test`, `cargo clippy`
- Node: `npm install`, `npm run build`, `npm test`
- Python: `python -m pytest`, `python -m mypy`

## Checklist Evolution

A healthy checklist evolves like this:

**Iteration 1** (Controller):
```markdown
# Project: REST API

# Requirements
- [ ] User management
- [ ] Authentication
- [ ] Data persistence

# Tasks
- [ ] Set up project structure
```

**Iteration 2** (Worker):
```markdown
# Project: REST API

# Requirements
- [ ] User management
- [ ] Authentication
- [ ] Data persistence

# Tasks
- [ ] Set up database schema
- [ ] Implement user CRUD endpoints
- [ ] Add authentication middleware
```
*(Committed: "Set up Rust project with Actix-web and PostgreSQL")*

**Iteration 3** (Controller):
```markdown
# Project: REST API

# Requirements
- [ ] User management
- [ ] Authentication
- [ ] Data persistence

# Tasks
- [~] Set up database schema
    - [ ] Add migration for password_reset_tokens table
- [ ] Implement user CRUD endpoints
- [ ] Add authentication middleware
```

**Final Iteration** (Controller):
```markdown
# Project: REST API

__ALL_TASKS_COMPLETE__
```

## Completion Detection

Completion detection varies by mode:

### Single-Checklist Mode (Token-Based)
Worker mode and controller mode share the same completion token (default: `__ALL_TASKS_COMPLETE__`), but the exit rules differ:

- **Worker mode**: **DEFAULT: Do NOT emit the token.** Most worker turns end without it. You may only emit the token when: (a) you've re-read the entire checklist, (b) there are ZERO `[ ]` items, (c) there are ZERO `[~]` items, (d) code builds cleanly, (e) all changes are committed. Completing one task does NOT mean emitting the token. afkcode treats the token as a stop request, then issues a confirmation prompt; the loop exits only if you emit the token again in that confirmation turn.
- **Controller mode**: Only the controller can emit the completion token. afkcode sends a verification prompt to confirm intent before exiting.

If anything remains incomplete or uncertain, do **not** emit the token.

### Multi-Checklist Mode (Scanner-Based)
Workers **NEVER** emit the completion token. Instead, afkcode uses deterministic scanning:

1. After each worker iteration, scanner checks all AGENTS.md files
2. Looks for incomplete markers: `[ ]`, `[~]`, `[ip]`, `[ip:XXXX]`
3. If all checklists have zero incomplete items, workers phase ends
4. If `--verify` is enabled, verifier runs to audit "complete" items
5. If `--spiral` is enabled and verifier finds work, workers restart

This approach ensures:
- No single worker can prematurely declare project-wide completion
- Completion is objective and deterministic
- Verifier can catch items that were marked complete but aren't actually done

## Summary

As an LLM working through afkcode:
1. **Respect Standing Orders** - They are immutable
2. **Be Incremental** - Small, testable changes
3. **Stay Organized** - Update checklist accurately
4. **Commit Regularly** - After each work session
5. **Compile Always** - Leave code in buildable state
6. **Communicate Clearly** - Update checklist for next iteration

Your goal is to maintain continuous forward progress while keeping the checklist accurate and the codebase functional.
