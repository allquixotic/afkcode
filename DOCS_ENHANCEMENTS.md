# Documentation Enhancements Summary

## Overview

Comprehensive documentation update to cover all afkcode features, with special focus on custom AGENTS.md usage and Standing Orders audit functionality.

## New Documentation Files

### 1. AGENTS_GUIDE.md (NEW)
Complete guide covering:
- What is AGENTS.md and why use it
- Core Standing Orders (all 9 rules explained in detail)
- Project-specific Standing Orders
- Standing Orders audit process
  - When to run it
  - How it works (target detection, LLM alignment, auto-commit)
  - Configuration options
- Creating and using AGENTS.md
  - Structure and templates
  - Custom locations
  - Benefits and use cases
- AGENTS.md vs checklist Standing Orders
- Example workflows
- Configuration reference
- Best practices
- Troubleshooting

### 2. DOCS_ENHANCEMENTS.md (THIS FILE)
Summary of all documentation changes

## Enhanced Documentation Files

### afkcode.toml.example
**Added:**
- `mode` configuration option (worker/controller)
- `skip_audit` option with explanation (default: true)
- `orders_path` option for custom AGENTS.md locations
- `commit_audit` option for controlling auto-commit behavior
- `gemini_model`, `claude_model`, `codex_model` options
- Updated default tools to `gemini,codex,claude`
- References to AGENTS_GUIDE.md

### README.md
**Major Updates:**

#### Features Section
- Added Gemini support mention
- Added Model Selection feature
- Added Custom AGENTS.md feature
- Updated Multi-LLM Support to include Gemini

#### Run Command Documentation
- Fixed `--skip-audit` → `--run-audit` (corrected flag name)
- Added `--gemini-model`, `--claude-model`, `--codex-model` flags
- Updated default tools from `codex,claude` to `gemini,codex,claude`
- Added reference to AGENTS_GUIDE.md for `--audit-orders-path`

#### Run Command Examples
- Updated fallback chain examples (Gemini first)
- Added model selection examples
- Fixed audit flag examples (`--run-audit` instead of `--skip-audit`)
- Added custom AGENTS.md location example

#### Generate/Add-Batch/Update Commands
- Updated default tools to include Gemini
- Added model selection flags to all LLM-using commands

#### Standing Orders Section
- Completely rewritten as "Standing Orders and Custom AGENTS.md"
- Added Core Standing Orders Summary (all 9 rules)
- Added Custom AGENTS.md quick start guide
- Added prominent link to AGENTS_GUIDE.md

#### Configuration Section
- Added missing config options to examples:
  - `mode`
  - `skip_audit`
  - `orders_path`
  - `commit_audit`
  - Model selection options
- Updated default tools in examples to include Gemini

#### LLM Backend Configuration
- Added Gemini CLI as first supported tool
- Added model selection documentation for all tools
- Added Gemini unattended configuration section
- Updated "Using Default Fallback" examples
- Added "Model Selection Examples" section
- Updated tool order examples

#### Troubleshooting
- Added Gemini to "Tool not found" examples
- Updated fallback detection examples with Gemini
- Added "Standing Orders being modified unexpectedly" troubleshooting
- Added "Want to use custom AGENTS.md location" troubleshooting

#### Recent Changes
- Added Gemini support
- Added Model selection feature
- Added Custom AGENTS.md feature
- Added Audit disabled by default change
- Added Enhanced configuration options

#### Differences from Python Version
- Updated to include Gemini support
- Updated to mention custom AGENTS.md
- Updated to reflect model selection capabilities
- Added 9th difference (total now 9 items)

#### See Also Section
- Added AGENTS_GUIDE.md as first item with description
- Added afkcode.toml.example reference
- Added TESTING.md reference
- Clarified AGENTS.md is created via `--run-audit`

## Key Corrections Made

### Critical Bug Fixes in Documentation

1. **--skip-audit → --run-audit**
   - **Issue**: README documented `--skip-audit` flag which doesn't exist
   - **Fix**: Corrected to `--run-audit` (actual CLI flag)
   - **Impact**: Audit is now disabled by default, requires explicit flag to enable

2. **Default Tool Order**
   - **Issue**: Documentation showed `codex,claude` as defaults
   - **Fix**: Updated to `gemini,codex,claude` (actual defaults in code)
   - **Impact**: Users now know Gemini is tried first

3. **Missing Configuration Options**
   - **Issue**: Config file example missing many options
   - **Fix**: Added `mode`, `skip_audit`, `orders_path`, `commit_audit`, model options
   - **Impact**: Users can now fully configure afkcode via config file

4. **Model Selection Not Documented**
   - **Issue**: --gemini-model, --claude-model, --codex-model flags undocumented
   - **Fix**: Added comprehensive model selection documentation
   - **Impact**: Users can now specify custom models (e.g., gemini-2.5-pro, o3, opus)

## New Features Explained

### 1. Custom AGENTS.md Support
- Separate file for Standing Orders and LLM instructions
- Keeps checklists clean and focused on tasks
- Enables sharing one AGENTS.md across multiple checklists
- Better version control of behavioral rules
- Warp Agent Mode integration

### 2. Standing Orders Audit
- **Default behavior**: Disabled (requires `--run-audit`)
- **Target detection**: Automatic priority-based selection
  1. Explicit path via `--audit-orders-path`
  2. `AGENTS.md` in current directory
  3. Standing Orders section in checklist
- **LLM alignment**: Syncs core orders, preserves project-specific orders
- **Auto-commit**: Commits changes automatically (configurable)

### 3. Model Selection
- Per-tool model specification via CLI flags
- Per-tool model specification via config file
- Examples for common models:
  - Gemini: `gemini-2.5-pro`
  - Claude: `opus`, `sonnet`, `claude-sonnet-4-5-20250929`
  - Codex: `o3`, `o4-mini`

### 4. Gemini CLI Support
- Now the default first tool in fallback chain
- Automatic `--yolo` flag for unattended operation
- Rate limit detection
- Model selection support

## Documentation Structure Improvements

### Before
- Single README with all info mixed together
- Standing Orders briefly mentioned
- No guidance on AGENTS.md usage
- Missing configuration options

### After
- README: Quick reference and common tasks
- AGENTS_GUIDE.md: Comprehensive guide for advanced features
- afkcode.toml.example: Complete configuration reference
- Clear cross-references between documents
- Logical progression from basic to advanced topics

## Usage Examples Added

### AGENTS.md Workflows
```bash
# Setup for new project
afkcode init project.md --title "My Project"
touch AGENTS.md
afkcode run project.md --run-audit

# Use custom location
afkcode run project.md --run-audit --audit-orders-path docs/AGENTS.md

# Configure via config file
echo 'orders_path = "docs/AGENTS.md"' >> afkcode.toml
```

### Model Selection
```bash
# CLI flags
afkcode run project.md --tools gemini --gemini-model gemini-2.5-pro

# Config file
echo 'gemini_model = "gemini-2.5-pro"' >> afkcode.toml
afkcode run project.md
```

## Benefits of These Enhancements

1. **Accuracy**: All documented features match actual code behavior
2. **Completeness**: Every CLI flag and config option now documented
3. **Discoverability**: New users can find custom AGENTS.md feature easily
4. **Clarity**: Separate guide for complex topics (AGENTS_GUIDE.md)
5. **Troubleshooting**: Added common issues and solutions
6. **Examples**: Comprehensive examples for all major features
7. **Organization**: Clear document hierarchy and cross-references

## For New Users

Start with:
1. README.md - Installation and quick start
2. Run first project with defaults
3. Read "Standing Orders and Custom AGENTS.md" section
4. Explore AGENTS_GUIDE.md for advanced usage

## For Existing Users

Key changes to be aware of:
1. Audit now disabled by default (use `--run-audit`)
2. Gemini is now default first tool
3. Can specify custom models per tool
4. Can use AGENTS.md for Standing Orders
5. More config file options available

## Next Steps

Consider adding:
1. Video tutorial for AGENTS.md setup
2. More example AGENTS.md templates for common project types
3. Migration guide for Python version users
4. FAQ section
5. Comparison matrix of when to use checklist vs AGENTS.md for Standing Orders
