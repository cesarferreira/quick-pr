#!/usr/bin/env bash
set -euo pipefail
# cargo-release also runs hooks in dry-run mode; keep that mode read-only.
if [[ "${DRY_RUN:-true}" == "true" ]]; then
    git-cliff --tag "v${NEW_VERSION:?}" >/dev/null
else
    git-cliff --tag "v${NEW_VERSION:?}" --output CHANGELOG.md
fi
