#!/bin/bash
# afkcode Smoke Test Suite
# Tests all major commands with a simple contrived project

set -e  # Exit on error
set -u  # Exit on undefined variable

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Test configuration
TEST_DIR="$(mktemp -d)"
AFKCODE="${AFKCODE_BIN:-./target/release/afkcode}"
CLEANUP=${CLEANUP:-true}

# Test counters
TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

# Print functions
print_header() {
    echo -e "\n${BLUE}========================================${NC}"
    echo -e "${BLUE}$1${NC}"
    echo -e "${BLUE}========================================${NC}\n"
}

print_test() {
    echo -e "${YELLOW}[TEST $((TESTS_RUN + 1))]${NC} $1"
    TESTS_RUN=$((TESTS_RUN + 1))
}

print_pass() {
    echo -e "${GREEN}✓ PASS${NC}: $1"
    TESTS_PASSED=$((TESTS_PASSED + 1))
}

print_fail() {
    echo -e "${RED}✗ FAIL${NC}: $1"
    TESTS_FAILED=$((TESTS_FAILED + 1))
}

print_info() {
    echo -e "  ${BLUE}→${NC} $1"
}

# Cleanup function
cleanup() {
    if [ "$CLEANUP" = true ]; then
        print_info "Cleaning up test directory: $TEST_DIR"
        rm -rf "$TEST_DIR"
    else
        print_info "Test directory preserved at: $TEST_DIR"
    fi
}
trap cleanup EXIT

# Check afkcode binary exists
check_binary() {
    print_header "Checking afkcode binary"

    if [ ! -f "$AFKCODE" ]; then
        echo -e "${RED}Error: afkcode binary not found at $AFKCODE${NC}"
        echo "Build it with: cargo build --release"
        echo "Or set AFKCODE_BIN environment variable to the binary path"
        exit 1
    fi

    print_info "Found afkcode at: $AFKCODE"

    # Test help command
    if "$AFKCODE" --help > /dev/null 2>&1; then
        print_pass "afkcode --help works"
    else
        print_fail "afkcode --help failed"
        exit 1
    fi
}

# Test 1: init command
test_init() {
    print_header "Test 1: init command"

    local checklist="$TEST_DIR/init_test.md"

    print_test "init basic checklist"
    if "$AFKCODE" init "$checklist" --title "Test Project" > /dev/null 2>&1; then
        if [ -f "$checklist" ]; then
            print_pass "Created checklist file"
        else
            print_fail "Checklist file not created"
            return
        fi
    else
        print_fail "init command failed"
        return
    fi

    print_test "init checklist contains standing orders"
    if grep -q "STANDING ORDERS" "$checklist"; then
        print_pass "Standing orders present"
    else
        print_fail "Standing orders missing"
    fi

    print_test "init with examples"
    local checklist_examples="$TEST_DIR/init_examples.md"
    if "$AFKCODE" init "$checklist_examples" --title "Examples Project" --examples > /dev/null 2>&1; then
        if grep -q "# Tasks" "$checklist_examples" || grep -q "# Requirements" "$checklist_examples"; then
            print_pass "Examples included"
        else
            print_fail "Examples not included"
        fi
    else
        print_fail "init with --examples failed"
    fi
}

# Test 2: add command
test_add() {
    print_header "Test 2: add command"

    local checklist="$TEST_DIR/add_test.md"
    "$AFKCODE" init "$checklist" --title "Add Test" > /dev/null 2>&1

    print_test "add single item"
    if "$AFKCODE" add "$checklist" "Implement hello world function" > /dev/null 2>&1; then
        if grep -q "Implement hello world function" "$checklist"; then
            print_pass "Item added successfully"
        else
            print_fail "Item not found in checklist"
        fi
    else
        print_fail "add command failed"
    fi

    print_test "add sub-item"
    if "$AFKCODE" add "$checklist" "Write unit tests" --sub > /dev/null 2>&1; then
        if grep -q "    - \[ \] Write unit tests" "$checklist"; then
            print_pass "Sub-item added with correct indentation"
        else
            print_fail "Sub-item not properly indented"
        fi
    else
        print_fail "add --sub command failed"
    fi

    print_test "add to specific section"
    # First ensure there's a section
    echo -e "\n# Custom Section\n" >> "$checklist"
    if "$AFKCODE" add "$checklist" "Task in custom section" --section "Custom Section" > /dev/null 2>&1; then
        # Check if item appears after the section header and before the next header (or EOF)
        if grep -A 5 "# Custom Section" "$checklist" | grep -q "Task in custom section"; then
            print_pass "Item added to specific section"
        else
            print_fail "Item not in correct section"
        fi
    else
        print_fail "add --section command failed"
    fi
}

# Test 3: remove command
test_remove() {
    print_header "Test 3: remove command"

    local checklist="$TEST_DIR/remove_test.md"
    "$AFKCODE" init "$checklist" --title "Remove Test" > /dev/null 2>&1
    "$AFKCODE" add "$checklist" "Keep this task" > /dev/null 2>&1
    "$AFKCODE" add "$checklist" "REMOVE_ME task" > /dev/null 2>&1
    "$AFKCODE" add "$checklist" "Another keeper" > /dev/null 2>&1

    print_test "remove items with pattern"
    if "$AFKCODE" remove "$checklist" "REMOVE_ME" --yes > /dev/null 2>&1; then
        if ! grep -q "REMOVE_ME" "$checklist"; then
            print_pass "Item removed successfully"
        else
            print_fail "Item still present in checklist"
        fi

        if grep -q "Keep this task" "$checklist" && grep -q "Another keeper" "$checklist"; then
            print_pass "Other items preserved"
        else
            print_fail "Other items were incorrectly removed"
        fi
    else
        print_fail "remove command failed"
    fi
}

# Test 4: generate command (LLM-based)
test_generate() {
    print_header "Test 4: generate command (LLM)"

    local checklist="$TEST_DIR/generate_test.md"

    print_test "generate checklist from simple prompt"
    print_info "This test requires an LLM tool to be available (claude or codex)"

    # Use a very simple prompt to minimize LLM time/cost
    if timeout 120 "$AFKCODE" generate "$checklist" "Create a Python script that prints 'Hello World'" --tools claude 2>&1 | tee "$TEST_DIR/generate.log"; then
        if [ -f "$checklist" ]; then
            if grep -q "STANDING ORDERS" "$checklist"; then
                print_pass "Generated checklist with standing orders"
            else
                print_fail "Generated checklist missing standing orders"
            fi

            if grep -q "\[ \]" "$checklist"; then
                print_pass "Generated checklist contains tasks"
            else
                print_fail "Generated checklist has no tasks"
            fi
        else
            print_fail "Checklist file not created"
        fi
    else
        print_info "Generate command timed out or failed (LLM might not be available)"
        print_info "Skipping this test - check $TEST_DIR/generate.log for details"
    fi
}

# Test 5: add-batch command (LLM-based)
test_add_batch() {
    print_header "Test 5: add-batch command (LLM)"

    local checklist="$TEST_DIR/add_batch_test.md"
    "$AFKCODE" init "$checklist" --title "Batch Test" > /dev/null 2>&1

    print_test "add-batch with simple description"
    print_info "This test requires an LLM tool to be available (claude or codex)"

    local items_before=$(grep -c "\[ \]" "$checklist" || true)

    if timeout 120 "$AFKCODE" add-batch "$checklist" "Add error handling for file operations" --tools claude 2>&1 | tee "$TEST_DIR/add_batch.log"; then
        local items_after=$(grep -c "\[ \]" "$checklist" || true)

        if [ "$items_after" -gt "$items_before" ]; then
            print_pass "Items added to checklist (before: $items_before, after: $items_after)"
        else
            print_fail "No items were added"
        fi
    else
        print_info "add-batch command timed out or failed (LLM might not be available)"
        print_info "Skipping this test - check $TEST_DIR/add_batch.log for details"
    fi
}

# Test 6: update command (LLM-based)
test_update() {
    print_header "Test 6: update command (LLM)"

    local checklist="$TEST_DIR/update_test.md"
    "$AFKCODE" init "$checklist" --title "Update Test" > /dev/null 2>&1
    "$AFKCODE" add "$checklist" "Low priority task" > /dev/null 2>&1
    "$AFKCODE" add "$checklist" "High priority task" > /dev/null 2>&1

    print_test "update checklist with LLM instruction"
    print_info "This test requires an LLM tool to be available (claude or codex)"

    if timeout 120 "$AFKCODE" update "$checklist" "Add a section called 'Priority Tasks' at the top" --tools claude 2>&1 | tee "$TEST_DIR/update.log"; then
        if [ -f "${checklist}.bak" ]; then
            print_pass "Backup file created"
        else
            print_fail "Backup file not created"
        fi

        if grep -q "STANDING ORDERS" "$checklist"; then
            print_pass "Standing orders preserved"
        else
            print_fail "Standing orders were removed"
        fi
    else
        print_info "update command timed out or failed (LLM might not be available)"
        print_info "Skipping this test - check $TEST_DIR/update.log for details"
    fi
}

# Test 7: run command (minimal autonomous loop)
test_run() {
    print_header "Test 7: run command (minimal loop)"

    # Create a git repository for the test
    local project_dir="$TEST_DIR/run_project"
    mkdir -p "$project_dir"
    cd "$project_dir"

    git init > /dev/null 2>&1
    git config user.email "test@example.com"
    git config user.name "Test User"

    local checklist="$project_dir/checklist.md"

    # Create a simple checklist with one easy task
    "$AFKCODE" init "$checklist" --title "Hello World Python" > /dev/null 2>&1
    "$AFKCODE" add "$checklist" "Create hello.py that prints 'Hello World'" > /dev/null 2>&1

    print_test "run autonomous loop (1 iteration max)"
    print_info "This test requires an LLM tool and may take a minute"

    # We'll run the loop but kill it after a short time to prevent infinite running
    # In a real scenario, the controller should detect completion
    timeout 180 "$AFKCODE" run "$checklist" --tools claude --sleep-seconds 5 2>&1 | tee "$TEST_DIR/run.log" || true

    # Check if any git commits were made
    if git log --oneline > /dev/null 2>&1; then
        local commit_count=$(git log --oneline | wc -l)
        if [ "$commit_count" -gt 0 ]; then
            print_pass "Git commits were made ($commit_count commits)"
        else
            print_info "No git commits (LLM might not have completed a task)"
        fi
    else
        print_info "Could not check git log (test might have been too short)"
    fi

    # Check if checklist was modified
    if [ -f "$checklist" ]; then
        print_pass "Checklist still exists after run"
    else
        print_fail "Checklist was deleted"
    fi

    cd - > /dev/null
}

# Test 8: File format validation
test_file_format() {
    print_header "Test 8: File format validation"

    local checklist="$TEST_DIR/format_test.md"
    "$AFKCODE" init "$checklist" --title "Format Test" > /dev/null 2>&1

    print_test "checklist has proper markdown structure"

    local has_title=$(grep -c "^# " "$checklist" || true)
    if [ "$has_title" -ge 1 ]; then
        print_pass "Has H1 title"
    else
        print_fail "Missing H1 title"
    fi

    if grep -q "# STANDING ORDERS" "$checklist"; then
        print_pass "Has standing orders section"
    else
        print_fail "Missing standing orders section"
    fi

    # Count the standing orders (should be 9)
    local order_count=$(grep -c "^[0-9]\+\. " "$checklist" || true)
    if [ "$order_count" -eq 9 ]; then
        print_pass "Has all 9 standing orders"
    else
        print_fail "Expected 9 standing orders, found $order_count"
    fi
}

# Test 9: Edge cases
test_edge_cases() {
    print_header "Test 9: Edge cases"

    print_test "handle non-existent checklist"
    if ! "$AFKCODE" add "$TEST_DIR/nonexistent.md" "Test task" > /dev/null 2>&1; then
        print_pass "Correctly fails on non-existent checklist"
    else
        print_fail "Should fail on non-existent checklist"
    fi

    print_test "handle invalid command"
    if ! "$AFKCODE" invalid-command > /dev/null 2>&1; then
        print_pass "Correctly rejects invalid command"
    else
        print_fail "Should reject invalid command"
    fi

    print_test "init refuses to overwrite existing file"
    local existing="$TEST_DIR/existing.md"
    touch "$existing"
    if ! "$AFKCODE" init "$existing" --title "Test" > /dev/null 2>&1; then
        print_pass "Correctly refuses to overwrite"
    else
        print_fail "Should refuse to overwrite existing file"
    fi
}

# Main test execution
main() {
    print_header "afkcode Smoke Test Suite"
    print_info "Test directory: $TEST_DIR"
    print_info "afkcode binary: $AFKCODE"

    check_binary

    # Run all tests
    test_init
    test_add
    test_remove
    test_file_format
    test_edge_cases

    # LLM-based tests (may fail if no LLM available)
    print_header "LLM-Dependent Tests"
    print_info "The following tests require claude or codex to be available"
    print_info "They may be skipped if LLM tools are unavailable"

    test_generate
    test_add_batch
    test_update
    test_run

    # Summary
    print_header "Test Summary"
    echo -e "Total tests run: ${BLUE}$TESTS_RUN${NC}"
    echo -e "Tests passed:    ${GREEN}$TESTS_PASSED${NC}"
    echo -e "Tests failed:    ${RED}$TESTS_FAILED${NC}"

    if [ "$TESTS_FAILED" -eq 0 ]; then
        echo -e "\n${GREEN}All tests passed!${NC} ✓"
        exit 0
    else
        echo -e "\n${RED}Some tests failed${NC} ✗"
        exit 1
    fi
}

# Run main
main
