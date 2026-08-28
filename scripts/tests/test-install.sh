#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
TEST_ROOT="$(mktemp -d)"
trap 'rm -rf "${TEST_ROOT}"' EXIT

literal_password='pa\nINJECTED=yes'
ADMIN_PASSWORD="${literal_password}" bash "${REPO_ROOT}/install.sh" \
    --compose-dir "${TEST_ROOT}" \
    --skip-start >/dev/null

if [[ "$(grep -c '^ADMIN_PASSWORD=' "${TEST_ROOT}/.env")" -ne 1 ]]; then
    echo "install wrote an invalid ADMIN_PASSWORD entry" >&2
    exit 1
fi
grep -Fqx "ADMIN_PASSWORD=${literal_password}" "${TEST_ROOT}/.env"
if grep -Fqx 'INJECTED=yes' "${TEST_ROOT}/.env"; then
    echo "literal password escape injected a second env entry" >&2
    exit 1
fi

bash "${REPO_ROOT}/install.sh" \
    --compose-dir "${TEST_ROOT}" \
    --version v9.9.9 \
    --skip-start >/dev/null
grep -Fqx 'APP_IMAGE=ghcr.io/ryfinez/niffler:9.9.9' "${TEST_ROOT}/.env"

bash "${REPO_ROOT}/install.sh" \
    --compose-dir "${TEST_ROOT}" \
    --app-image registry.example.com/niffler:test \
    --skip-start >/dev/null
grep -Fqx 'APP_IMAGE=registry.example.com/niffler:test' "${TEST_ROOT}/.env"

echo "install.sh regression checks passed"
