#!/usr/bin/env bash
set -euo pipefail

for file in $CLAUDE_FILE_PATHS; do
    if [[ "$file" == *.rs ]]; then
        rustfmt "$file" 2>/dev/null || true
        break
    fi
done
