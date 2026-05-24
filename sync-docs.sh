#!/bin/bash
# Manually sync AGENTS.md from main repo to all worktrees.
# Run this after editing AGENTS.md in the main repo.
#
# Usage: bash sync-docs.sh

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
FILES_TO_SYNC="AGENTS.md"

echo "Syncing from: $SCRIPT_DIR"

# List all worktrees (skip the first line which is the main tree)
git worktree list --porcelain | grep '^worktree ' | sed 's/^worktree //' | while read -r wt; do
    # Skip the main working tree itself
    if [ "$wt" = "$SCRIPT_DIR" ]; then
        continue
    fi

    for file in $FILES_TO_SYNC; do
        if [ -f "$SCRIPT_DIR/$file" ]; then
            cp "$SCRIPT_DIR/$file" "$wt/$file" 2>/dev/null && \
                echo "  $file -> $(basename "$wt")"
        fi
    done
done

echo "Done."
