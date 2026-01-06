# Custom AGENTS.md Guide

## What is AGENTS.md?

`AGENTS.md` is an optional separate file for storing Standing Orders and other instructions for LLMs working on your project. This is particularly useful for:

- **Documentation separation**: Keep LLM instructions separate from human-readable documentation
- **Sharing across checklists**: Use one AGENTS.md for multiple project checklists
- **Version control**: Track changes to LLM behavior independently
- **Warp Agent Mode integration**: Store custom instructions that Warp's Agent Mode can reference

## Standing Orders

### Core Standing Orders

afkcode includes 9 core standing orders that govern LLM behavior:

1. **Minimal Information**: Checklist items contain only the minimum needed for an LLM to act
2. **Completion Handling**: Delete fully complete items; for partials, change `[ ]` to `[~]` and add sub-items
3. **Discovery**: Add newly discovered work as new items, succinctly
4. **Git Commit**: Before finishing a turn, run `git add` and `git commit` with descriptive message
5. **Immutability**: The Standing Orders section is immutable except during alignment
6. **No Manual Work**: Do not require or mention manual human steps or testing; prefer automation
7. **"Do the thing"**: Review checklist, pick an item, implement, update, build, fix errors, commit
8. **"Fix shit"**: Identify broken code/design, fix, update checklist, commit
9. **Stop Token Etiquette**: Emit completion token only when everything is done and committed

### Project-Specific Standing Orders

You can add project-specific rules under a `# Project Standing Orders` heading:

```markdown
# STANDING ORDERS - DO NOT DELETE

[9 core orders here...]

# Project Standing Orders

10. All code must follow the project's ESLint configuration
11. Use TypeScript strict mode for all new files
12. Run `npm test` before each commit
13. Follow naming convention: camelCase for functions, PascalCase for classes
```

## Standing Orders Audit

### Overview

The audit process ensures your project's Standing Orders stay synchronized with afkcode's core rules. **The audit is disabled by default** to avoid unwanted modifications.

### When to Run the Audit

Enable the audit when:

- **Initial project setup**: When first using afkcode with an existing checklist
- **After afkcode upgrades**: When core Standing Orders change  
- **Manual synchronization**: When you want to update project-specific orders

```bash
# Run audit on demand
afkcode run project.md --run-audit
```

### How the Audit Works

1. **Target Detection**: Automatically finds Standing Orders in priority order:
   - Custom path specified via `--audit-orders-path`
   - `AGENTS.md` file in current directory
   - Standing Orders section within the checklist itself

2. **LLM Alignment**: Uses an LLM to:
   - Insert/update the 9 core Standing Orders
   - Preserve project-specific orders under "# Project Standing Orders"
   - Remove duplicates and obsolete rules

3. **Automatic Commit**: Changes are committed to git (unless `commit_audit = false`)

### Audit Configuration

**Via CLI:**
```bash
# Run audit with default target (AGENTS.md or checklist)
afkcode run project.md --run-audit

# Run audit with custom path
afkcode run project.md --run-audit --audit-orders-path docs/AGENTS.md
```

**Via config file:**
```toml
# afkcode.toml

# Skip audit by default (recommended)
skip_audit = true

# Custom path for Standing Orders
orders_path = "AGENTS.md"

# Disable automatic git commit of audit changes
commit_audit = false
```

## Creating and Using AGENTS.md

### Creating AGENTS.md

Create an `AGENTS.md` file in your project root:

```bash
touch AGENTS.md
```

Initialize it with Standing Orders:

```bash
# Run audit to populate AGENTS.md
afkcode run project.md --run-audit
```

If `AGENTS.md` exists, afkcode automatically uses it instead of the Standing Orders section in your checklist.

### AGENTS.md Structure

A typical AGENTS.md contains:

```markdown
# Project Name - Technical Documentation for LLM Agents

> **Note**: This document is designed for LLM consumption. For human-readable documentation, see README.md.

## Purpose

[Brief description of the project]

# STANDING ORDERS - DO NOT DELETE

[9 core standing orders from afkcode]

# Project Standing Orders

[Your project-specific rules]

## Architecture

[Technical details for LLMs]

## Development Workflow

[Build commands, test procedures, etc.]

## Integration Points

[External systems, APIs, dependencies]
```

### Using Custom AGENTS.md Locations

If your AGENTS.md is not in the current directory:

**Via CLI:**
```bash
afkcode run project.md --run-audit --audit-orders-path docs/AGENTS.md
```

**Via config file:**
```toml
# afkcode.toml
orders_path = "docs/AGENTS.md"
```

**Per-project configs:**
```bash
# Project A uses default location
cd project-a
echo 'orders_path = "AGENTS.md"' > afkcode.toml

# Project B uses custom location
cd ../project-b
echo 'orders_path = "docs/llm/AGENTS.md"' > afkcode.toml
```

### Benefits of AGENTS.md

1. **Cleaner checklists**: Keep task lists focused on tasks, not meta-instructions
2. **Reusability**: Share one AGENTS.md across multiple checklist files
3. **Better version control**: Track behavioral changes separately from task changes
4. **Warp integration**: Reference via `@AGENTS.md` in Warp's Agent Mode
5. **Documentation clarity**: Separate human docs (README.md) from LLM docs (AGENTS.md)

### AGENTS.md vs Checklist Standing Orders

Afkcode automatically chooses the audit target in this priority order:

1. **Explicit path** via `--audit-orders-path` or `orders_path` config
2. **AGENTS.md** in current directory (if it exists)
3. **Standing Orders section** within the checklist file itself

If you have both AGENTS.md and Standing Orders in your checklist, AGENTS.md takes precedence during audit.

## Example Workflow

### Setup for New Project

```bash
# Create checklist
afkcode init project.md --title "My Project"

# Create AGENTS.md
touch AGENTS.md

# Run audit to populate AGENTS.md with core orders
afkcode run project.md --run-audit

# Add project-specific orders manually
echo "\n# Project Standing Orders\n\n10. Follow ESLint rules" >> AGENTS.md

# Start development
afkcode run project.md
```

### Setup with Existing Project

```bash
# If you already have custom instructions in your checklist
afkcode run existing_project.md --run-audit

# This will:
# 1. Find Standing Orders in existing_project.md
# 2. Align them with core orders
# 3. Preserve any project-specific orders
# 4. Commit changes
```

### Using AGENTS.md with Warp Agent Mode

When working in Warp's Agent Mode, you can reference your AGENTS.md:

```
@AGENTS.md Do the thing.
```

This gives Warp's LLM access to your Standing Orders and project-specific instructions.

## Configuration Reference

### CLI Flags

- `--run-audit`: Run the Standing Orders alignment audit (disabled by default)
- `--audit-orders-path <PATH>`: Override the audit target file path

### Config File Options

```toml
# Skip audit by default (recommended to avoid unwanted changes)
# Default: true
skip_audit = true

# Custom path for Standing Orders file
# If not specified, uses AGENTS.md in current directory (if exists)
# or the Standing Orders section within the checklist
# orders_path = "AGENTS.md"
# orders_path = "docs/AGENTS.md"

# Automatically commit audit changes to git
# Default: true
commit_audit = true
```

## Best Practices

1. **Keep AGENTS.md in version control**: Track how your LLM instructions evolve
2. **Review audit changes**: Always review what the audit changes before committing
3. **Don't run audit frequently**: Only run when needed (setup, upgrades, manual sync)
4. **Separate concerns**: Use AGENTS.md for LLM instructions, README.md for humans
5. **Be specific**: Make project-specific orders actionable and clear
6. **Avoid contradictions**: Project orders shouldn't contradict core orders

## Troubleshooting

**Problem**: Audit modifying my custom orders

```bash
# Ensure your custom orders are under "# Project Standing Orders" heading
# The audit preserves this section while updating core orders
```

**Problem**: Want to skip audit entirely

```toml
# Add to afkcode.toml
skip_audit = true
```

Or never pass `--run-audit` flag (audit is disabled by default).

**Problem**: AGENTS.md not being used

```bash
# Check if AGENTS.md exists in current directory
ls -la AGENTS.md

# Or specify explicit path
afkcode run project.md --run-audit --audit-orders-path ./AGENTS.md
```

**Problem**: Audit changes not committed

```toml
# Check commit_audit setting
# Add to afkcode.toml
commit_audit = true
```
