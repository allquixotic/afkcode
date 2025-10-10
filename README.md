# afkcode - LLM-Powered Checklist Management & Autonomous Development Loop

A Rust port and enhancement of `codex_loop.py` that provides a Swiss Army knife for managing project checklists and running autonomous development loops with LLM CLI tools.

## Features

- **Autonomous Development Loop**: Alternates between controller and worker prompts to make continuous progress
- **Checklist Generation**: Create checklists from high-level prompts using LLM
- **Checklist Management**: Add, remove, and update checklist items with or without LLM assistance
- **Standing Orders**: Built-in project invariants ensure consistent LLM behavior
- **Multi-LLM Support**: Automatic fallback between Codex CLI and Claude Code on rate limits
- **Smart Rate Limit Handling**: Automatically switches to backup LLM tool when quota exhausted

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

Runs the controller/worker loop against a checklist until completion or all LLM tools exhausted.

```bash
afkcode run <checklist> [OPTIONS]

Options:
  --controller-prompt <TEMPLATE>     Custom controller prompt
  --worker-prompt <TEMPLATE>         Custom worker prompt
  --completion-token <TOKEN>         Completion detection string
  --sleep-seconds <N>                Delay between iterations (default: 15)
  --tools <TOOLS>                    Comma-separated list of LLM tools (default: codex,claude)
```

**Examples:**
```bash
# With default fallback (Codex → Claude)
afkcode run project.md

# Claude only (no fallback)
afkcode run project.md --tools claude

# Codex only (no fallback)
afkcode run project.md --tools codex

# Custom sleep time with fallback
afkcode run project.md --tools codex,claude --sleep-seconds 30
```

**How Fallback Works:**
1. Starts with first tool in list (e.g., `codex`)
2. If rate limit detected, automatically switches to next tool (`claude`)
3. Continues development loop seamlessly
4. Only exits if all tools exhausted or completion detected

**Exit Conditions:**
- Controller emits completion token (default: `__ALL_TASKS_COMPLETE__`)
- All LLM tools exhausted due to rate limits
- User presses Ctrl+C

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
  --tools <TOOLS>        Comma-separated list of LLM tools (default: codex,claude)
  
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
  --tools <TOOLS>        Comma-separated list of LLM tools (default: codex,claude)
  
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
  --tools <TOOLS>        Comma-separated list of LLM tools (default: codex,claude)
  
```

**Example:**
```bash
afkcode update project.md \
  "Reorganize tasks by priority, moving critical items to the top"
```

**Note:** Creates a `.md.bak` backup before updating.

## Standing Orders

All generated and initialized checklists include these standing orders (invariants):

1. All additions must be minimal information for LLM understanding
2. Complete items are removed; partial items get `~` checkbox with sub-items
3. New items discovered during work are added succinctly
4. Git commits are made before finishing work
5. Standing orders cannot be altered or deleted
6. No manual human effort or testing references allowed
7. "Do the thing" command: review, pick task, implement, update, compile, commit
8. "Fix shit" command: identify broken code/design, fix, update, commit

These invariants ensure consistent LLM behavior across autonomous development sessions.

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
# LLM tools to use (comma-separated: codex, claude)
# Default: "codex,claude"
tools = "codex,claude"

# Sleep duration between LLM calls in seconds
# Default: 15
sleep_seconds = 20

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

afkcode supports these LLM CLI tools with automatic fallback:

1. **Codex CLI** (`codex`)
   - Command: `codex exec`
   - Rate limit detection: "rate limit reached", "429", "rate_limit_error"

2. **Claude Code** (`claude`)
   - Command: `claude --print`
   - Rate limit detection: "usage limit reached", "limit will reset", "429"

### Using Default Fallback

```bash
# Tries Codex first, falls back to Claude on rate limit
afkcode run checklist.md
afkcode run checklist.md --tools codex,claude
```

### Using Single Tool

```bash
# Codex only (no fallback)
afkcode run checklist.md --tools codex

# Claude only (no fallback)
afkcode run checklist.md --tools claude
```

### Custom Tool Order

```bash
# Try Claude first, fall back to Codex
afkcode run checklist.md --tools claude,codex
```

**Note**: Both tools must be installed and available on your PATH for fallback to work.

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
# Verify both tools are installed
which codex && which claude

# Check rate limit detection messages in output
# Should see "Rate limit detected for codex"
# Followed by "Switching to fallback tool: claude"
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

### Bug Fixes

- **Fixed sub-item formatting**: Sub-items now properly include the dash prefix (`    - [ ]` instead of `    [ ]`)
- **Fixed section-specific add**: Items added with `--section` now correctly appear in the specified section
- **Fixed standing orders preservation**: The `update` command now automatically restores standing orders if the LLM removes them

### New Features

- **Configuration file support**: Added `afkcode.toml` support for persistent configuration
- **Global --config flag**: Specify custom configuration file paths
- **Configuration precedence**: CLI args override config file values, which override defaults

## Differences from Python Version

1. **Subcommands**: Not just a loop - full checklist management toolkit
2. **Standing Orders**: Built-in invariants from webdev2.md
3. **Automatic Fallback**: Seamless switching between LLM tools on rate limits
4. **Supported Tools**: Codex CLI and Claude Code with tool-specific rate limit detection
5. **Better Error Handling**: Comprehensive error messages with anyhow
6. **Type Safety**: Compile-time guarantees for correctness
7. **Cross-Platform**: Works on Linux, macOS, Windows
8. **Configuration Files**: TOML-based configuration for project-specific settings

## Contributing

Contributions are welcome! Please open an issue or pull request at:
https://github.com/allquixotic/afkcode

## License

Licensed under the Apache License, Version 2.0. See LICENSE.txt for details.

Copyright (c) 2025 Sean McNamara <smcnam@gmail.com>

## See Also

- `AGENTS.md` - LLM-focused documentation (not for human reading)
- `example_checklist.md` - Sample checklist structure
- GitHub: https://github.com/allquixotic/afkcode
