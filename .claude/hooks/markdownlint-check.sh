#!/bin/bash
# Pre-commit hook: blocks git commit if markdownlint finds violations.
cd "$(git rev-parse --show-toplevel)"
if ! markdownlint '**/*.md' > /dev/null 2>&1; then
    markdownlint '**/*.md' 2>&1 >&2
    echo '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"markdownlint failed — fix violations before committing"}}'
fi
