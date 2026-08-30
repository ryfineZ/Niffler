#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
TEST_ROOT="$(mktemp -d)"
trap 'rm -rf "${TEST_ROOT}"' EXIT

INSTALL_ROOT="${TEST_ROOT}/fresh"
mkdir -p "${INSTALL_ROOT}"

literal_password='pa\nINJECTED=yes'
ADMIN_PASSWORD="${literal_password}" bash "${REPO_ROOT}/install.sh" \
    --compose-dir "${INSTALL_ROOT}" \
    --skip-start >/dev/null

if [[ "$(grep -c '^ADMIN_PASSWORD=' "${INSTALL_ROOT}/.env")" -ne 1 ]]; then
    echo "install wrote an invalid ADMIN_PASSWORD entry" >&2
    exit 1
fi
grep -Fqx "ADMIN_PASSWORD=${literal_password}" "${INSTALL_ROOT}/.env"
if grep -Fqx 'INJECTED=yes' "${INSTALL_ROOT}/.env"; then
    echo "literal password escape injected a second env entry" >&2
    exit 1
fi

bash "${REPO_ROOT}/install.sh" \
    --compose-dir "${INSTALL_ROOT}" \
    --version v9.9.9 \
    --skip-start >/dev/null
grep -Fqx 'APP_IMAGE=ghcr.io/ryfinez/niffler:9.9.9' "${INSTALL_ROOT}/.env"

bash "${REPO_ROOT}/install.sh" \
    --compose-dir "${INSTALL_ROOT}" \
    --app-image registry.example.com/niffler:test \
    --skip-start >/dev/null
grep -Fqx 'APP_IMAGE=registry.example.com/niffler:test' "${INSTALL_ROOT}/.env"

RELEASE_ROOT="${TEST_ROOT}/release"
RELEASE_INSTALL="${RELEASE_ROOT}/install.sh"
RELEASE_DEPLOYMENT="${TEST_ROOT}/release-deployment"
mkdir -p "${RELEASE_ROOT}" "${RELEASE_DEPLOYMENT}"
sed \
    -e 's/^VERSION="${AETHER_VERSION:-}"/VERSION="${AETHER_VERSION:-v9.8.7}"/' \
    "${REPO_ROOT}/install.sh" >"${RELEASE_INSTALL}"
chmod 0755 "${RELEASE_INSTALL}"
install -m 0644 "${REPO_ROOT}/docker-compose.yml" "${RELEASE_ROOT}/docker-compose.yml"
install -m 0644 "${REPO_ROOT}/.env.example" "${RELEASE_ROOT}/.env.example"
install -m 0755 "${REPO_ROOT}/generate_keys.sh" "${RELEASE_ROOT}/generate_keys.sh"
printf 'APP_IMAGE=ghcr.io/ryfinez/niffler:old\n' >"${RELEASE_DEPLOYMENT}/.env"

bash "${RELEASE_INSTALL}" \
    --compose-dir "${RELEASE_DEPLOYMENT}" \
    --skip-start >/dev/null
grep -Fqx 'APP_IMAGE=ghcr.io/ryfinez/niffler:9.8.7' "${RELEASE_DEPLOYMENT}/.env"

bash "${RELEASE_INSTALL}" \
    --compose-dir "${RELEASE_DEPLOYMENT}" \
    --app-image registry.example.com/niffler:release-override \
    --skip-start >/dev/null
grep -Fqx 'APP_IMAGE=registry.example.com/niffler:release-override' "${RELEASE_DEPLOYMENT}/.env"

LEGACY_SQLITE_ROOT="${TEST_ROOT}/legacy-sqlite"
mkdir -p "${LEGACY_SQLITE_ROOT}"
printf 'AETHER_DATABASE_DRIVER=sqlite\nAETHER_DATABASE_URL=sqlite:///app/data/aether.db\n' >"${LEGACY_SQLITE_ROOT}/.env"
printf 'legacy compose sentinel\n' >"${LEGACY_SQLITE_ROOT}/docker-compose.yml"
if bash "${REPO_ROOT}/install.sh" \
    --compose-dir "${LEGACY_SQLITE_ROOT}" \
    --skip-start >"${LEGACY_SQLITE_ROOT}/output.log" 2>&1; then
    echo "install accepted an existing SQLite deployment" >&2
    exit 1
fi
grep -Fq 'legacy SQLite database configuration detected' "${LEGACY_SQLITE_ROOT}/output.log"
grep -Fqx 'legacy compose sentinel' "${LEGACY_SQLITE_ROOT}/docker-compose.yml"
if [[ -e "${LEGACY_SQLITE_ROOT}/.env.example" ]]; then
    echo "install replaced deployment files before rejecting SQLite" >&2
    exit 1
fi

LEGACY_MYSQL_ROOT="${TEST_ROOT}/legacy-mysql"
LEGACY_MYSQL_ENV="${TEST_ROOT}/legacy-mysql.env"
mkdir -p "${LEGACY_MYSQL_ROOT}"
printf 'DATABASE_URL=mysql://aether:secret@mysql/aether\n' >"${LEGACY_MYSQL_ENV}"
if bash "${REPO_ROOT}/install.sh" \
    --compose-dir "${LEGACY_MYSQL_ROOT}" \
    --env-file "${LEGACY_MYSQL_ENV}" \
    --skip-start >"${LEGACY_MYSQL_ROOT}/output.log" 2>&1; then
    echo "install accepted a MySQL environment seed" >&2
    exit 1
fi
grep -Fq 'legacy MySQL/MariaDB database configuration detected' "${LEGACY_MYSQL_ROOT}/output.log"
if [[ -e "${LEGACY_MYSQL_ROOT}/docker-compose.yml" ]]; then
    echo "install replaced deployment files before rejecting MySQL" >&2
    exit 1
fi

LEGACY_MYSQL_COMPOSE_ROOT="${TEST_ROOT}/legacy-mysql-compose"
mkdir -p "${LEGACY_MYSQL_COMPOSE_ROOT}"
printf 'services:\n  database:\n    image: docker.io/library/mysql@sha256:deadbeef\n' >"${LEGACY_MYSQL_COMPOSE_ROOT}/docker-compose.yml"
if bash "${REPO_ROOT}/install.sh" \
    --compose-dir "${LEGACY_MYSQL_COMPOSE_ROOT}" \
    --skip-start >"${LEGACY_MYSQL_COMPOSE_ROOT}/output.log" 2>&1; then
    echo "install accepted an existing MySQL Compose deployment" >&2
    exit 1
fi
grep -Fq 'legacy MySQL/MariaDB database configuration detected' "${LEGACY_MYSQL_COMPOSE_ROOT}/output.log"
grep -Fqx 'services:' "${LEGACY_MYSQL_COMPOSE_ROOT}/docker-compose.yml"
if [[ -e "${LEGACY_MYSQL_COMPOSE_ROOT}/.env.example" ]]; then
    echo "install replaced deployment files before rejecting MySQL Compose" >&2
    exit 1
fi

LEGACY_SERVICE_ROOT="${TEST_ROOT}/legacy-service-target"
LEGACY_SERVICE_ENV="${TEST_ROOT}/legacy-system-service.env"
mkdir -p "${LEGACY_SERVICE_ROOT}"
printf 'AETHER_DATABASE_DRIVER=sqlite\nDATABASE_URL=sqlite:///opt/aether/data/aether.db\n' >"${LEGACY_SERVICE_ENV}"
if AETHER_LEGACY_SYSTEM_ENV_PATH="${LEGACY_SERVICE_ENV}" bash "${REPO_ROOT}/install.sh" \
    --compose-dir "${LEGACY_SERVICE_ROOT}" \
    --skip-start >"${LEGACY_SERVICE_ROOT}/output.log" 2>&1; then
    echo "install accepted a legacy SQLite system-service deployment" >&2
    exit 1
fi
grep -Fq 'legacy SQLite database configuration detected' "${LEGACY_SERVICE_ROOT}/output.log"
if [[ -e "${LEGACY_SERVICE_ROOT}/docker-compose.yml" ]]; then
    echo "install replaced deployment files before rejecting the legacy system service" >&2
    exit 1
fi

echo "install.sh regression checks passed"
