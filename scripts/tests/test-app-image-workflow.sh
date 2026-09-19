#!/bin/bash

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
WORKFLOW="$SCRIPT_DIR/../../.github/workflows/app-image.yml"

test -f "$WORKFLOW"
grep -Fq "workflow_dispatch:" "$WORKFLOW"
grep -Fq "      - test" "$WORKFLOW"
grep -Fq "cancel-in-progress: \${{ github.ref_name != 'test' }}" "$WORKFLOW"
grep -Fq "if: github.ref_name == 'test'" "$WORKFLOW"
grep -Fq "name: test" "$WORKFLOW"
grep -Fq 'url: ${{ vars.MYLINGWEAVE_PUBLIC_URL }}' "$WORKFLOW"
grep -Fq 'SSH_USER: ${{ vars.MYLINGWEAVE_USER }}' "$WORKFLOW"
grep -Fq 'REMOTE_DIR: ${{ vars.MYLINGWEAVE_REMOTE_DIR }}' "$WORKFLOW"
grep -Fq 'SOURCE_HEALTH_URL: ${{ vars.MYLINGWEAVE_SOURCE_HEALTH_URL }}' "$WORKFLOW"
grep -Fq "name: Download test image artifact" "$WORKFLOW"
grep -Fq "uses: actions/download-artifact@v8" "$WORKFLOW"
DOWNLOAD_BLOCK="$(sed -n '/- name: Download test image artifact/,/- name: Setup SSH/p' "$WORKFLOW")"
grep -Fq "name: niffler-app-linux-amd64" <<< "$DOWNLOAD_BLOCK"
grep -Fq 'path: ${{ runner.temp }}/niffler-app-artifact' <<< "$DOWNLOAD_BLOCK"
grep -Fq -- '--artifact-file "${RUNNER_TEMP}/niffler-app-artifact/niffler-app-linux-amd64.tar"' "$WORKFLOW"
grep -Fq -- '--test-deployment' "$WORKFLOW"
grep -Fq -- '--source-health-url "${SOURCE_HEALTH_URL}"' "$WORKFLOW"
grep -Fq -- '--public-health-url "${{ vars.MYLINGWEAVE_PUBLIC_URL }}"' "$WORKFLOW"
if grep -Fq -- '--run-id "${{ github.run_id }}"' "$WORKFLOW"; then
    echo "test deployment must not query its own in-progress workflow run" >&2
    exit 1
fi

echo "test deployment workflow checks passed"
