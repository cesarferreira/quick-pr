#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ "$#" -lt 1 || "$#" -gt 2 || ( "$#" == 2 && "$2" != --execute ) ]]; then
    echo "Usage: $0 <level-or-version> [--execute]" >&2
    exit 2
fi
if [[ "$(uname -s)" != Darwin ]]; then
    echo "Release Quick PR from macOS so the crate can be verified." >&2
    exit 1
fi
if [[ -n "$(git status --porcelain)" ]]; then
    echo "Commit all changes before releasing (including a dry run)." >&2
    exit 1
fi
if [[ "${2:-}" == --execute ]]; then
    # A crate upload is public and permanent. Never change repository visibility here.
    visibility="$(gh repo view cesarferreira/quick-pr --json visibility --jq .visibility)"
    if [[ "$visibility" != PUBLIC ]]; then
        echo "Repository is private. Review its contents and make it public before publishing a public crate." >&2
        exit 1
    fi
fi
make test check
args=("$1")
if [[ "${2:-}" == --execute ]]; then
    args+=(--execute --no-confirm)
fi
cargo release "${args[@]}"
