#!/bin/bash
# Pre-commit hook: blocks git commit if cargo fmt --check fails.
cd "$(git rev-parse --show-toplevel)"
if ! cargo fmt --check > /dev/null 2>&1; then
    echo '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"cargo fmt --check failed — run: cargo fmt"}}'
fi
