# afkcode - LLM-Powered Checklist Management & Autonomous Development Loop

A Rust port and enhancement of `codex_loop.py` that provides a Swiss Army knife for managing project checklists and running autonomous development loops with LLM CLI tools.

## Features

- **Autonomous Development Loop**: Worker-only loop by default, with optional controller/worker alternation
- **Parallel LLM Execution**: Run multiple LLM instances simultaneously with staggered warmup delays to maximize throughput
- **Gimme Mode**: Automatic work item checkout from AGENTS.md files - LLMs get assigned specific tasks to prevent conflicts
- **Checklist Generation**: Create checklists from high-level prompts using LLM
- **Checklist Management**: Add, remove, and update checklist items with or without LLM assistance
- **Standing Orders**: Built-in project invariants ensure consistent LLM behavior
- **Standing Orders Audit**: One-time alignment keeps in-repo Standing Orders synced with the core rules
- **Multi-LLM Support**: Automatic fallback between Gemini, Codex CLI, Claude Code, and Warp Agent API on rate limits
- **Model Selection**: Specify custom models for each LLM tool (e.g., gemini-2.5-pro, o3, opus)
- **Custom AGENTS.md**: Separate file for LLM instructions and Standing Orders (see AGENTS_GUIDE.md)
- **Smart Rate Limit Handling**: Automatically switches to backup LLM tool when quota exhausted with temporary 5-minute squelching
- **Completion Token Verification**: LLM confirms intentional completion to prevent accidental loop exits
- **Output Logging**: Automatically streams all LLM output and console messages to a configurable log file during run mode
- **Failed LLM Recovery**: Automatically restores work items if an LLM subprocess crashes or hits rate limits

## Installation

```bash
cd afkcode
cargo build --release
```

The binary will be in `target/release/afkcode`.

## Quick Start

```bash
# Create a new checklist with examples
afkcode init my_project.md --title "My Project" --examples

# Generate a checklist from a description
afkcode generate api_project.md "Build a REST API for user management with PostgreSQL"

# Run the autonomous development loop
afkcode run my_project.md

# Add a single task
afkcode add my_project.md "Implement authentication" --section "Tasks"

# Remove completed items
afkcode remove my_project.md "DONE" --yes
```

## Commands

### `run` - Autonomous Development Loop

Runs the worker loop against a checklist until completion or all LLM tools are exhausted. Use `--mode controller` to opt into the legacy controller/worker alternation.

```bash
afkcode run <checklist> [OPTIONS]

Options:
  --controller-prompt <TEMPLATE>     Custom controller prompt
  --worker-prompt <TEMPLATE>         Custom worker prompt
  --completion-token <TOKEN>         Completion detection string
  --sleep-seconds <N>                Delay between iterations (default: 15)
  --mode <worker|controller>         Loop mode (default: worker)
  --run-audit                        Run the Standing Orders alignment audit (disabled by default)
  --audit-orders-path <PATH>         Override the Standing Orders audit target file (see AGENTS_GUIDE.md)
  --tools <TOOLS>                    Comma-separated list of LLM tools (default: gemini,codex,claude)
  --log-file <PATH>                  Log file path for streaming output (default: afkcode.log)
  --gemini-model <MODEL>             Model to use for Gemini CLI (e.g., gemini-2.5-pro)
  --claude-model <MODEL>             Model to use for Claude CLI (e.g., sonnet, opus)
  --codex-model <MODEL>              Model to use for Codex CLI (e.g., o3, o4-mini)

Parallel Execution Options:
  --num-instances <N>                Number of parallel LLM instances (default: 1)
  --warmup-delay <SECONDS>           Delay between launching instances (default: 30, 0 to disable)
  --no-gimme                         Disable gimme mode (work item checkout)
  --gimme-path <PATH>                Base path for AGENTS.md file search (default: current directory)
  --items-per-instance <N>           Number of work items each instance checks out (default: 1)

Warp Agent API requires WARP_API_KEY environment variable or warp_api_key in config.
See "Warp Agent API" section below for details.
```

**Examples:**
```bash
# With default fallback (Gemini → Codex → Claude)
afkcode run project.md

# Specific tool with custom model
afkcode run project.md --tools gemini --gemini-model gemini-2.5-pro

# Claude with opus model
afkcode run project.md --tools claude --claude-model opus

# Controller/worker alternation
afkcode run project.md --mode controller

# Run the Standing Orders audit (see AGENTS_GUIDE.md)
afkcode run project.md --run-audit

# Use custom AGENTS.md location
afkcode run project.md --run-audit --audit-orders-path docs/AGENTS.md

# Custom sleep time with multiple tools
afkcode run project.md --tools codex,claude --sleep-seconds 30

# Run 3 parallel LLM instances with 30-second warmup delay between each
afkcode run project.md --num-instances 3 --warmup-delay 30

# Parallel execution with gimme mode (each instance gets its own work item)
afkcode run project.md --num-instances 4 --items-per-instance 2

# Disable gimme mode (all instances work on same checklist)
afkcode run project.md --num-instances 2 --no-gimme
```

**How Fallback Works:**
1. Starts with first tool in list (default: `gemini`)
2. If rate limit detected, automatically switches to next tool (e.g., `codex`, then `claude`)
3. Rate-limited tools are temporarily squelched for 5 minutes
4. After 5 minutes, the system automatically tries the most preferred tool again
5. Continues development loop seamlessly
6. Only exits if all tools exhausted or completion detected

**Parallel Execution:**

Run multiple LLM instances simultaneously to maximize throughput:

1. **Staggered Launch**: Instances launch with configurable warmup delay (default 30s) to prevent API rate limit spikes
2. **Independent Fallback**: Each instance has its own LlmToolChain with separate rate limit tracking
3. **Coordinated Shutdown**: When any instance confirms completion (stop token twice), all instances finish their current iteration and exit
4. **Gimme Mode**: By default, each instance checks out work items from AGENTS.md files, preventing multiple LLMs from working on the same task

**Gimme Mode (Work Item Checkout):**

Gimme mode automatically assigns work items to each LLM instance:

1. Parses all `AGENTS.md` files under the search path
2. Selects items marked `[ ]` (incomplete) - configurable to include `[x]` or `[BLOCKED]`
3. Marks selected items as `[ip:XXXX]` with unique checkout IDs
4. Injects work items into each instance's prompt
5. **Failed LLM Recovery**: If an LLM crashes or hits rate limits, its work items are automatically restored to `[ ]` using the checkout ID

Example AGENTS.md workflow:
```markdown
# Before checkout
- [ ] Implement user authentication
- [ ] Add rate limiting
- [ ] Write unit tests

# After checkout (instance 0 gets first item)
- [ip:a3f7] Implement user authentication
- [ ] Add rate limiting
- [ ] Write unit tests
```

**Completion Token Verification:**
- **Worker mode**: The worker's stdout is scanned (case-insensitive) for the configured `completion_token`. If detected, afkcode runs a confirmation turn using a dedicated prompt. The loop exits only when the token appears again in that confirmation response; otherwise work resumes normally.
- **Controller mode**: Unchanged. When the controller emits the completion token, afkcode asks the LLM to re-confirm intent before exiting.

**Exit Conditions:**
- Worker mode: Token appears in consecutive worker and confirmation turns
- Controller mode: Controller emits completion token (default: `__ALL_TASKS_COMPLETE__`) **and** the verification prompt confirms intent
- All LLM tools exhausted due to rate limits
- User presses Ctrl+C

**Output Logging:**
All console output during run mode (LLM responses, status messages, errors) is automatically streamed to a log file (default: `afkcode.log`). This can be customized via the `--log-file` CLI argument or the `log_file` config option. The log file uses buffered writing to maintain responsiveness while capturing all output for later review.

> Checklist hygiene (short bullets, removing completed items, using sub-items for partials) is enforced by the Standing Orders that live in your repository; afkcode does not rewrite checklist content during worker turns.

### `init` - Create New Checklist

Creates a bare checklist with standing orders.

```bash
afkcode init <checklist> [OPTIONS]

Options:
  -t, --title <TITLE>     Project title (default: "Project Checklist")
  -e, --examples          Include example sections
```

**Example:**
```bash
afkcode init webapp.md --title "Web Application" --examples
```

### `generate` - Generate Checklist from Prompt

Uses LLM to generate a complete checklist from a high-level description.

```bash
afkcode generate <checklist> <prompt> [OPTIONS]

Options:
  --tools <TOOLS>        Comma-separated list of LLM tools (default: gemini,codex,claude)
  --gemini-model <MODEL> Model to use for Gemini CLI (e.g., gemini-2.5-pro)
  --claude-model <MODEL> Model to use for Claude CLI (e.g., sonnet, opus)
  --codex-model <MODEL>  Model to use for Codex CLI (e.g., o3, o4-mini)
  
```

**Example:**
```bash
afkcode generate microservice.md \
  "Create a microservice in Rust using Actix-web and PostgreSQL for product inventory tracking"
```

### `add` - Add Single Item

Adds one checklist item without LLM assistance.

```bash
afkcode add <checklist> <item> [OPTIONS]

Options:
  --sub                  Add as sub-item (indented)
  -s, --section <NAME>   Add to specific section
```

**Examples:**
```bash
# Add to end of file
afkcode add project.md "Write unit tests"

# Add as sub-item
afkcode add project.md "Test error handling" --sub

# Add to specific section
afkcode add project.md "Deploy to staging" --section "Deployment"
```

### `add-batch` - Add Multiple Items via LLM

Uses LLM to expand a high-level description into multiple checklist items.

```bash
afkcode add-batch <checklist> <description> [OPTIONS]

Options:
  --tools <TOOLS>        Comma-separated list of LLM tools (default: gemini,codex,claude)
  --gemini-model <MODEL> Model to use for Gemini CLI
  --claude-model <MODEL> Model to use for Claude CLI
  --codex-model <MODEL>  Model to use for Codex CLI
```

**Example:**
```bash
afkcode add-batch project.md \
  "Add comprehensive error handling to the authentication module"
```

### `remove` - Remove Items

Removes items matching a pattern (substring search).

```bash
afkcode remove <checklist> <pattern> [OPTIONS]

Options:
  -y, --yes    Skip confirmation prompt
```

**Examples:**
```bash
# Interactive removal
afkcode remove project.md "COMPLETED"

# Non-interactive
afkcode remove project.md "old feature" --yes
```

### `update` - Update Checklist via LLM

Uses LLM to update the checklist according to instructions.

```bash
afkcode update <checklist> <instruction> [OPTIONS]

Options:
  --tools <TOOLS>        Comma-separated list of LLM tools (default: gemini,codex,claude)
  --gemini-model <MODEL> Model to use for Gemini CLI
  --claude-model <MODEL> Model to use for Claude CLI
  --codex-model <MODEL>  Model to use for Codex CLI
```

**Example:**
```bash
afkcode update project.md \
  "Reorganize tasks by priority, moving critical items to the top"
```

**Note:** Creates a `.md.bak` backup before updating.

## Standing Orders and Custom AGENTS.md

Afkcode uses **Standing Orders** - a set of 9 immutable rules that govern LLM behavior during autonomous development. These ensure consistent, predictable behavior across sessions.

### Core Standing Orders Summary

1. **Minimal Information**: Checklist items contain only the minimum needed for LLM action
2. **Completion Handling**: Delete complete items; use `[~]` with sub-items for partials
3. **Discovery**: Add newly discovered work succinctly
4. **Git Commit**: Always commit before finishing a turn
5. **Immutability**: Standing Orders can't be altered except during audit
6. **No Manual Work**: Prefer automation over manual steps
7. **"Do the thing"**: Review, pick, implement, update, build, commit
8. **"Fix shit"**: Identify and fix broken code/design, update, commit
9. **Stop Token Etiquette**: Only emit completion token when truly done

### Custom AGENTS.md

You can store Standing Orders and other LLM instructions in a separate `AGENTS.md` file. This provides:

- **Cleaner checklists**: Separate task lists from meta-instructions
- **Reusability**: Share one AGENTS.md across multiple checklists
- **Better version control**: Track behavioral changes separately
- **Warp integration**: Reference via `@AGENTS.md` in Warp's Agent Mode

**Quick Start with AGENTS.md:**

```bash
# Create AGENTS.md
touch AGENTS.md

# Populate it with Standing Orders
afkcode run project.md --run-audit

# Now AGENTS.md contains core orders + space for project-specific rules
```

**For complete documentation on Standing Orders, audit process, and AGENTS.md usage, see [`AGENTS_GUIDE.md`](AGENTS_GUIDE.md).**

## Workflow Examples

### Example 1: Starting a New Project

```bash
# Generate initial checklist
afkcode generate api.md "Build RESTful API for blog platform with Rust and PostgreSQL"

# Review the generated checklist
cat api.md

# Run autonomous development
afkcode run api.md --sleep-seconds 20
```

### Example 2: Adding Features

```bash
# Add high-level requirement
afkcode add api.md "Add rate limiting to API endpoints" --section "Requirements"

# Expand into tasks
afkcode add-batch api.md \
  "Implement rate limiting using token bucket algorithm with Redis backend"

# Continue development
afkcode run api.md
```

### Example 3: Cleanup and Maintenance

```bash
# Remove completed items
afkcode remove api.md "DONE" --yes

# Reorganize
afkcode update api.md \
  "Group related tasks together and add section headers for API, Database, and Testing"
```

## Custom Prompts

### Controller Prompt

The controller reviews progress and assigns tasks. Default:

```
You are the controller in an autonomous development loop.
Study the shared checklist in @{checklist}, summarize current progress, and update it as needed.
Assign the next actionable task to the worker so momentum continues.
If—and only if—all high-level requirements and every checklist item are fully satisfied,
output {completion_token} on a line by itself at the very end of your reply;
otherwise, do not print that string.
```

### Worker Prompt

The worker implements tasks. Default:

```
@{checklist} Do the thing.
```

This assumes your checklist defines what "Do the thing" means (see Standing Orders #7).

### Custom Example

```bash
afkcode run project.md \
  --controller-prompt "Review @{checklist} and decide what's next. Output {completion_token} when done." \
  --worker-prompt "@{checklist} Implement the highest priority incomplete task."
```

## Configuration

### Configuration File Support

afkcode supports TOML configuration files to avoid repeating common CLI arguments. Configuration precedence (highest to lowest):

1. **CLI arguments** (always win)
2. **Config file** (if present)
3. **Built-in defaults** (fallback)

### Default Configuration File

afkcode automatically loads `afkcode.toml` from the current directory if it exists. You can also specify a custom config file:

```bash
afkcode --config /path/to/custom.toml run checklist.md
```

### Configuration File Format

Create an `afkcode.toml` file in your project directory. See `afkcode.toml.example` for a complete example with all available options.

```toml
# LLM tools to use (comma-separated: gemini, codex, claude)
# Default: "gemini,codex,claude"
tools = "gemini,codex,claude"

# Sleep duration between LLM calls in seconds
# Default: 15
sleep_seconds = 20

# Log file path for streaming output during run mode
# Default: "afkcode.log"
log_file = "afkcode.log"

# Controller prompt template
# Default: built-in template (see below)
controller_prompt = """
You are the controller in an autonomous development loop.
Study the shared checklist in @{checklist}, summarize current progress, and update it as needed.
Assign the next actionable task to the worker so momentum continues.
If—and only if—all high-level requirements and every checklist item are fully satisfied, output {completion_token} on a line by itself at the very end of your reply; otherwise, do not print that string.
"""

# Worker prompt template
# Default: "@{checklist} Do the thing."
worker_prompt = "@{checklist} Do the thing."

# Completion detection token
# Default: "__ALL_TASKS_COMPLETE__"
completion_token = "__ALL_TASKS_COMPLETE__"

# Run mode (worker or controller)
# Default: "worker"
# mode = "worker"

# Standing Orders audit configuration (see AGENTS_GUIDE.md)
# skip_audit = true
# orders_path = "AGENTS.md"
# commit_audit = true

# Model selection for LLM tools
# gemini_model = "gemini-2.5-pro"
# claude_model = "opus"
# codex_model = "o3"

# Parallel execution settings
# num_instances = 3           # Number of parallel LLM instances
# warmup_delay = 30           # Seconds between launching instances
# gimme_mode = true           # Enable work item checkout (default: true)
# gimme_base_path = "."       # Base path for AGENTS.md search
# gimme_items_per_instance = 1  # Work items each instance checks out
```

### Configuration Examples

**Example 1**: Use Claude by default with longer sleep time
```toml
# afkcode.toml
tools = "claude"
sleep_seconds = 30
```

Then simply run:
```bash
afkcode run checklist.md  # Uses Claude with 30s sleep automatically
```

**Example 2**: Override config with CLI arguments
```toml
# afkcode.toml
tools = "claude"
sleep_seconds = 30
```

```bash
# CLI args override config file
afkcode run checklist.md --tools codex --sleep-seconds 10
# Uses Codex with 10s sleep, not Claude with 30s
```

**Example 3**: Project-specific configurations
```bash
# Different config for each project
cd project-a
echo 'tools = "codex"' > afkcode.toml
afkcode run checklist.md  # Uses Codex

cd ../project-b
echo 'tools = "claude"' > afkcode.toml
afkcode run checklist.md  # Uses Claude
```

## LLM Backend Configuration

### Supported Tools

afkcode supports these LLM CLI tools and APIs with automatic fallback:

1. **Gemini CLI** (`gemini`) - **Default first choice**
   - Command: `gemini --yolo`
   - Flags automatically added by afkcode for unattended operation
   - Rate limit detection: "rate limit", "429", "quota"
   - Model selection: `--gemini-model gemini-2.5-pro`

2. **Codex CLI** (`codex`)
   - Command: `codex exec`
   - Configuration: Requires `~/.codex/config.toml` with `approval_policy = "never"`
   - Rate limit detection: "rate limit reached", "429", "rate_limit_error"
   - Model selection: `--codex-model o3` or `--codex-model o4-mini`

3. **Claude Code** (`claude`)
   - Command: `claude --print --dangerously-skip-permissions`
   - Flags automatically added by afkcode for unattended operation
   - Rate limit detection: "usage limit reached", "limit will reset", "429"
   - Model selection: `--claude-model opus` or `--claude-model sonnet`

4. **Warp Agent API** (`warp`) - **HTTP-based, multi-model**
   - Type: REST API (not CLI-based)
   - API endpoint: `https://app.warp.dev/api/v1`
   - Authentication: Bearer token via `WARP_API_KEY` environment variable or config
   - Rate limit detection: "rate limit", "429", "quota exceeded"
   - **Supports many models** via `warp_model` config:
     - **Claude**: `claude-sonnet-4-5`, `claude-opus-4-1`, `claude-haiku-4-5`, `claude-sonnet-4`
     - **OpenAI**: `gpt-5`, `gpt-5-1` (with low/medium/high reasoning modes)
     - **Google**: `gemini-3-pro`, `gemini-2-5-pro`
     - **z.ai**: `glm-4-6`
     - **Auto modes**: `auto-cost-efficient`, `auto-responsiveness`

### Using Default Fallback

```bash
# Tries Gemini first, then Codex, then Claude on rate limits
afkcode run checklist.md
afkcode run checklist.md --tools gemini,codex,claude
```

### Using Single Tool

```bash
# Gemini only (no fallback)
afkcode run checklist.md --tools gemini

# Gemini with specific model
afkcode run checklist.md --tools gemini --gemini-model gemini-2.5-pro

# Codex only with specific model
afkcode run checklist.md --tools codex --codex-model o3

# Claude only with specific model
afkcode run checklist.md --tools claude --claude-model opus
```

### Custom Tool Order

```bash
# Try Claude first, fall back to Codex, then Gemini
afkcode run checklist.md --tools claude,codex,gemini

# Try Codex only, no Gemini
afkcode run checklist.md --tools codex
```

### Model Selection Examples

```bash
# Use specific models for each tool
afkcode run checklist.md \
  --tools gemini,claude \
  --gemini-model gemini-2.5-pro \
  --claude-model opus

# Configure via config file
echo 'gemini_model = "gemini-2.5-pro"' >> afkcode.toml
echo 'claude_model = "sonnet"' >> afkcode.toml
afkcode run checklist.md
```

**Note**: CLI tools (gemini, codex, claude) must be installed and available on your PATH. Warp Agent API only requires an API key.

### Warp Agent API

Warp Agent API is a powerful option that provides access to multiple LLM models through a single HTTP API.

**Key Benefits:**
- **Multi-model access**: Switch between Claude, GPT, Gemini, and more via configuration
- **No CLI installation**: Just needs an API key
- **Unified billing**: One account for all models
- **Latest models**: Access to Claude 4.5 Sonnet, GPT-5, Gemini 3 Pro, etc.
- **Auto modes**: Let Warp choose the best model automatically

**Setup:**

1. Get your API key from [Warp](https://app.warp.dev)
2. Set environment variable:
   ```bash
   export WARP_API_KEY="your-key-here"
   ```
3. Or add to `afkcode.toml`:
   ```toml
   warp_api_key = "your-key-here"
   warp_model = "claude-sonnet-4-5"  # or any supported model
   ```

**Usage:**
```bash
# Use Warp Agent API
afkcode run checklist.md --tools warp

# With specific model
echo 'warp_model = "gpt-5"' >> afkcode.toml
afkcode run checklist.md --tools warp

# As fallback after Gemini
afkcode run checklist.md --tools gemini,warp
```

**Available Models:**

Warp Agent API supports a curated set of top LLMs:

- **Claude (Anthropic)**:
  - `claude-sonnet-4-5` - Latest Sonnet model
  - `claude-opus-4-1` - Most capable Claude model
  - `claude-haiku-4-5` - Fast and efficient
  - `claude-sonnet-4` - Previous generation

- **GPT (OpenAI)**:
  - `gpt-5` - Latest GPT model
  - `gpt-5-1` - GPT-5.1 with configurable reasoning

- **Gemini (Google)**:
  - `gemini-3-pro` - Latest Gemini
  - `gemini-2-5-pro` - Previous generation

- **GLM (z.ai)**:
  - `glm-4-6` - Hosted by Fireworks AI

- **Auto Modes**:
  - `auto-cost-efficient` - Optimizes for lower credit consumption
  - `auto-responsiveness` - Prioritizes highest quality and speed

See [Warp's model documentation](https://docs.warp.dev/agents/using-agents/model-choice) for the latest model list.

### Unattended Configuration

For autonomous operation, afkcode requires all LLM tools to be configured for unattended/automatic execution without prompts.

#### Gemini CLI Configuration

Gemini CLI is configured with the `--yolo` flag for unattended operation. afkcode automatically passes this flag:

```bash
gemini --yolo
```

- `--yolo`: Enables fully unattended execution without approval prompts

**Note**: Ensure Gemini CLI is installed and authenticated. No additional configuration file is needed.

#### Codex CLI Configuration

Codex CLI is configured via `~/.codex/config.toml`. Required settings for unattended operation:

```toml
approval_policy = "never"
sandbox_mode    = "danger-full-access"
```

This disables all approval prompts and grants full system access. The `approval_policy = "never"` setting ensures Codex executes all commands automatically without asking for confirmation.

#### Claude Code Configuration

Claude Code uses CLI flags for unattended operation. afkcode automatically passes the following flags:

```bash
claude --print --dangerously-skip-permissions
```

- `--print`: Non-interactive output mode
- `--dangerously-skip-permissions`: Enables "Safe YOLO mode" for fully unattended execution without approval prompts

**Security Note**: These settings grant the LLM unrestricted access to your system. Only use in trusted development environments. Consider running in an isolated container or VM for additional safety.

## Tips & Best Practices

1. **Be Specific**: Write detailed, actionable tasks in your checklist
2. **Small Increments**: Break large features into small, testable chunks
3. **Regular Commits**: Standing orders ensure commits happen automatically
4. **Monitor Progress**: Check git log periodically to review autonomous changes
5. **Rate Limits**: Increase `--sleep-seconds` if hitting rate limits frequently
6. **Backup**: Run `git status` before starting to ensure clean state
7. **Review**: Always review LLM-generated code before deploying

## Safety Features

- **Backup Creation**: `update` command creates `.md.bak` backups
- **Confirmation Prompts**: `remove` asks for confirmation (unless `--yes`)
- **Rate Limit Detection**: Automatically stops on rate limit
- **Ctrl+C Handling**: Graceful shutdown on interrupt

## Troubleshooting

**Problem**: Tool not found error
```bash
# Make sure your LLM tools are installed and on PATH
which gemini
which codex
which claude

# Or use only the tool you have installed
afkcode run checklist.md --tools claude
```

**Problem**: Loop exits immediately
```bash
# Check LLM command works independently
codex exec   # Should start Codex
claude --print "test"  # Should output response

# Try with single tool to debug
afkcode run checklist.md --tools codex
```

**Problem**: All tools rate limited
```bash
# Increase sleep time to reduce API usage
afkcode run checklist.md --sleep-seconds 60

# Use only one tool and wait for reset
afkcode run checklist.md --tools claude
```

**Problem**: Generated checklist has wrong structure
```bash
# Try more specific prompt
afkcode generate project.md "Detailed description with technology stack, architecture, and specific features..."

# Or try a different tool
afkcode generate project.md "..." --tools claude
```

**Problem**: Fallback not working
```bash
# Verify tools are installed
which gemini && which codex && which claude

# Check rate limit detection messages in output
# Should see "Rate limit detected for gemini"
# Followed by "Switching to fallback tool: codex"
```

**Problem**: Standing Orders being modified unexpectedly
```bash
# Audit is disabled by default to prevent unwanted changes
# Only run audit when you explicitly want to sync Standing Orders
afkcode run project.md --run-audit

# To completely disable audit in config:
echo 'skip_audit = true' >> afkcode.toml
```

**Problem**: Want to use custom AGENTS.md location
```bash
# Specify custom path via CLI
afkcode run project.md --run-audit --audit-orders-path docs/AGENTS.md

# Or via config file
echo 'orders_path = "docs/AGENTS.md"' >> afkcode.toml

# See AGENTS_GUIDE.md for complete documentation
```

## Development

### Building

```bash
cargo build
```

### Running Tests

```bash
cargo test
```

### Running from Source

```bash
cargo run -- run checklist.md
cargo run -- init new.md --examples
cargo run -- --help
```

## Recent Changes

### New Features (Latest)

- **Parallel LLM Execution**: Run multiple LLM instances simultaneously with staggered warmup delays (`--num-instances`, `--warmup-delay`)
- **Gimme Mode**: Automatic work item checkout from AGENTS.md files - each LLM instance gets assigned specific tasks to prevent conflicts
- **Failed LLM Recovery**: Automatically restores work items to `[ ]` if an LLM subprocess crashes or hits rate limits, using unique checkout IDs (`[ip:XXXX]`)
- **Coordinated Shutdown**: When any LLM confirms completion, all instances finish their current iteration and exit gracefully
- **Independent Fallback Tracking**: Each parallel instance has its own LlmToolChain for separate rate limit tracking
- **Warp Agent API Support**: HTTP-based access to multiple LLM models (Claude 4.5, GPT-5, Gemini 3 Pro, etc.) through Warp's unified API
- **Multi-Model Selection via Warp**: Single API key provides access to Claude, OpenAI, Google, and z.ai models
- **Gemini Support**: Gemini CLI is now the default first tool in the fallback chain
- **Model Selection**: Specify custom models for each LLM tool (--gemini-model, --claude-model, --codex-model, warp_model)
- **Custom AGENTS.md**: Comprehensive support for separate Standing Orders files (see AGENTS_GUIDE.md)
- **Audit Disabled by Default**: Standing Orders audit now requires --run-audit flag to prevent unwanted changes
- **Enhanced Configuration**: Added skip_audit, orders_path, commit_audit, warp_api_key, warp_model, and model selection config options
- **Temporary Tool Squelching**: Rate-limited tools are temporarily disabled for 5 minutes, then automatically retry
- **Completion Token Verification**: LLM confirms intentional completion to prevent false positives

### Bug Fixes

- **Fixed sub-item formatting**: Sub-items now properly include the dash prefix (`    - [ ]` instead of `    [ ]`)
- **Fixed section-specific add**: Items added with `--section` now correctly appear in the specified section
- **Fixed standing orders preservation**: The `update` command now automatically restores standing orders if the LLM removes them

### Previous Features

- **Configuration file support**: Added `afkcode.toml` support for persistent configuration
- **Global --config flag**: Specify custom configuration file paths
- **Configuration precedence**: CLI args override config file values, which override defaults

## Differences from Python Version

1. **Subcommands**: Not just a loop - full checklist management toolkit
2. **Standing Orders**: Built-in invariants with audit support and custom AGENTS.md
3. **Automatic Fallback**: Seamless switching between LLM tools on rate limits
4. **Supported Tools**: Gemini, Codex CLI, Claude Code, and Warp Agent API with tool-specific rate limit detection
5. **Warp Agent API**: HTTP-based multi-model access (Claude 4.5, GPT-5, Gemini 3 Pro, GLM 4.6, Auto modes)
6. **Model Selection**: Specify custom models per tool (e.g., gemini-2.5-pro, o3, opus, claude-sonnet-4-5)
6. **Better Error Handling**: Comprehensive error messages with anyhow
7. **Type Safety**: Compile-time guarantees for correctness
8. **Cross-Platform**: Works on Linux, macOS, Windows
9. **Configuration Files**: TOML-based configuration for project-specific settings

## Contributing

Contributions are welcome! Please open an issue or pull request at:
https://github.com/allquixotic/afkcode

## License

Licensed under the Apache License, Version 2.0. See LICENSE.txt for details.

Copyright (c) 2025 Sean McNamara <smcnam@gmail.com>

## See Also

- [`AGENTS_GUIDE.md`](AGENTS_GUIDE.md) - Complete guide to Custom AGENTS.md and Standing Orders audit
- `AGENTS.md` - Template for LLM-focused project documentation (created via `--run-audit`)
- `example_checklist.md` - Sample checklist structure
- `afkcode.toml.example` - Complete configuration file example
- `TESTING.md` - Testing documentation and smoke test suite
- GitHub: https://github.com/allquixotic/afkcode
