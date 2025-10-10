# Hello World Python Project

# STANDING ORDERS - DO NOT DELETE

1. All additions to this checklist must contain only the minimum information needed for an LLM to understand the task. Avoid verbose descriptions.

2. When you complete a to-do item: if it is 100% complete, delete it from the checklist entirely. If it is only partially complete, change its checkbox to `[~]` and add sub-items beneath it describing the remaining work.

3. If, while implementing a task, you discover new items of work, add them to the checklist in the appropriate section. Be succinct.

4. Before finishing your work session, execute `git add` and `git commit` with a descriptive message summarizing the changes you made.

5. Never alter, delete, or rewrite the "STANDING ORDERS" section. This section is immutable.

6. Checklist items must never require manual human effort or mention testing that cannot be automated. Focus only on design activities, coding activities, and automated processes that an LLM can execute autonomously.

7. When you receive the prompt ending in "Do the thing.", follow this workflow: (1) Review all remaining to-do items, (2) Arbitrarily pick an important incomplete item, (3) Implement it fully or partially, (4) Update the checklist (remove complete items, add sub-items to partial items, add newly discovered items), (5) Run build/compile commands for affected projects and fix any errors, (6) Execute `git add` and `git commit` with a descriptive message.

8. When you receive an instruction to fix problems (e.g., "fix shit"), follow this workflow: (1) Identify to-do items or known issues related to broken code, incomplete implementations, compilation errors, or other problems, (2) Fix those problems, (3) Update the checklist to reflect the fixes, (4) Execute `git add` and `git commit`.

# High-Level Requirements

- [ ] Create a Python script that prints "Hello, World!"
- [ ] Script should be executable
- [ ] Add basic error handling

# Tasks

- [ ] Create hello.py file
- [ ] Implement main() function that prints greeting
- [ ] Add shebang line for direct execution
- [ ] Add if __name__ == "__main__" guard
- [ ] Make file executable with chmod +x

# Notes

This is a minimal test project for validating afkcode functionality. The goal is simplicity and speed, not complexity.
