# Testing afkcode

## Smoke Test Suite

The `smoke_test.sh` script provides comprehensive end-to-end testing of afkcode's functionality.

### Running the Tests

```bash
# Build afkcode first
cargo build --release

# Run all tests
./smoke_test.sh

# Preserve test directory for inspection
CLEANUP=false ./smoke_test.sh

# Use custom binary location
AFKCODE_BIN=/path/to/afkcode ./smoke_test.sh
```

### Test Coverage

The smoke test suite covers:

1. **init command**
   - Basic checklist creation
   - Title customization
   - Example sections

2. **add command**
   - Single item addition
   - Sub-item indentation
   - Section-specific additions

3. **remove command**
   - Pattern-based removal
   - Preservation of non-matching items

4. **File format validation**
   - Markdown structure
   - Standing orders presence
   - Proper formatting

5. **Edge cases**
   - Non-existent files
   - Invalid commands
   - Overwrite protection

6. **LLM-dependent tests** (require claude or codex)
   - generate command
   - add-batch command
   - update command
   - run command (autonomous loop)

### LLM Requirements

Tests 6-9 require an LLM tool to be available:
- **Claude Code**: `claude` command on PATH
- **Codex CLI**: `codex` command on PATH

If no LLM is available, these tests will be skipped with informational messages.

### Test Output

The script produces colored output:
- 🟦 **Blue**: Headers and informational messages
- 🟡 **Yellow**: Test descriptions
- 🟢 **Green**: Passed tests
- 🔴 **Red**: Failed tests

### Test Artifacts

By default, all test files are created in a temporary directory and cleaned up after tests complete.

To preserve test artifacts for inspection:

```bash
CLEANUP=false ./smoke_test.sh
# Check output for test directory location
```

### Simple Contrived Project

The smoke tests use a "Hello World Python script" as the contrived project because:
- It's simple enough for LLMs to handle quickly
- It exercises all afkcode functionality
- It requires minimal tokens/API usage
- It can be completed in a short time

This ensures the tests focus on **afkcode's functionality** rather than LLM performance.

### Exit Codes

- `0`: All tests passed
- `1`: One or more tests failed
- `1`: afkcode binary not found

### CI/CD Integration

The smoke test script is designed for CI/CD integration:

```bash
# In your CI pipeline
cargo build --release
./smoke_test.sh
```

For CI environments without LLM access, the LLM-dependent tests will be gracefully skipped.

### Troubleshooting

**Binary not found:**
```bash
# Ensure you've built afkcode
cargo build --release

# Or specify binary location
AFKCODE_BIN=./target/debug/afkcode ./smoke_test.sh
```

**LLM tests timing out:**
- The tests use 120-180 second timeouts
- If your LLM is slow, tests may be skipped
- This is expected and safe - non-LLM tests will still run

**Permission denied:**
```bash
chmod +x smoke_test.sh
```

### Development Testing

During development, run tests frequently:

```bash
# Quick development cycle
cargo build && ./smoke_test.sh

# With verbose cargo output
cargo build --release --verbose && ./smoke_test.sh
```

### Adding New Tests

To add new tests to the suite:

1. Create a new function `test_your_feature()`
2. Use the `print_test`, `print_pass`, `print_fail` helpers
3. Call your function from `main()`
4. Increment test counters appropriately

Example:
```bash
test_your_feature() {
    print_header "Test X: Your Feature"

    print_test "feature does something"
    if your_test_logic; then
        print_pass "It works!"
    else
        print_fail "It doesn't work"
    fi
}
```
