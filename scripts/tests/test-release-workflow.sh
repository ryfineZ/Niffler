#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKFLOW="${SCRIPT_DIR}/../../.github/workflows/release.yml"

grep -Fq "if: needs.preflight.outputs.publish == 'true' || github.event_name == 'workflow_dispatch'" "${WORKFLOW}"
grep -Fq 'needs: [preflight, docker]' "${WORKFLOW}"
grep -Fq 'DOCKER_METADATA_SHORT_SHA_LENGTH: 7' "${WORKFLOW}"
grep -Fq 'VERSION="${GITHUB_SHA::7}"' "${WORKFLOW}"

if grep -Fq 'VERSION="snapshot-${GITHUB_SHA::7}"' "${WORKFLOW}"; then
    echo "manual release bundle still points at an unpublished snapshot image" >&2
    exit 1
fi

echo "release workflow regression checks passed"
