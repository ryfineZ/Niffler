#!/usr/bin/env bash
set -euo pipefail

REPO="${AETHER_REPO:-ryfineZ/Niffler}"
SOURCE_REF="${AETHER_SOURCE_REF:-main}"
COMPOSE_DIR="${AETHER_COMPOSE_DIR:-$(pwd)}"
MODE="${AETHER_INSTALL_MODE:-compose}"
APP_IMAGE="${AETHER_APP_IMAGE:-}"
VERSION="${AETHER_VERSION:-}"
APP_IMAGE_OVERRIDE_REQUESTED="false"
VERSION_OVERRIDE_REQUESTED="false"
ENV_SOURCE=""
SKIP_START="false"

if [[ -n "${AETHER_APP_IMAGE:-}" ]]; then
    APP_IMAGE_OVERRIDE_REQUESTED="true"
fi
if [[ -n "${AETHER_VERSION:-}" ]]; then
    VERSION_OVERRIDE_REQUESTED="true"
fi

usage() {
    cat <<'EOF'
Usage: install.sh [options]

Install Niffler with Docker Compose, PostgreSQL, and Redis.

Options:
  --mode compose       Deployment mode; only compose is supported
  --compose-dir PATH   Deployment directory (default: current directory)
  --env-file PATH      Seed the deployment from an existing env file
  --repo OWNER/REPO    GitHub source repository (default: ryfineZ/Niffler)
  --source-ref REF     Branch or tag used for deployment files (default: main)
  --app-image IMAGE    Container image override
  --version VERSION    Use ghcr.io/ryfinez/niffler:VERSION
  --skip-start         Prepare files without starting the deployment
  -h, --help           Show this help

Environment overrides:
  AETHER_REPO, AETHER_SOURCE_REF, AETHER_INSTALL_MODE, AETHER_COMPOSE_DIR
  AETHER_APP_IMAGE, AETHER_VERSION, ADMIN_PASSWORD
EOF
}

die() {
    echo "ERROR: $*" >&2
    exit 1
}

info() {
    echo ">>> $*" >&2
}

warn() {
    echo "WARNING: $*" >&2
}

parse_args() {
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --mode)
                [[ $# -ge 2 ]] || die "--mode requires a value"
                MODE="$2"
                shift 2
                ;;
            --compose-dir)
                [[ $# -ge 2 ]] || die "--compose-dir requires a path"
                COMPOSE_DIR="$2"
                shift 2
                ;;
            --env-file)
                [[ $# -ge 2 ]] || die "--env-file requires a path"
                ENV_SOURCE="$2"
                shift 2
                ;;
            --repo)
                [[ $# -ge 2 ]] || die "--repo requires OWNER/REPO"
                REPO="$2"
                shift 2
                ;;
            --source-ref)
                [[ $# -ge 2 ]] || die "--source-ref requires a ref"
                SOURCE_REF="$2"
                shift 2
                ;;
            --app-image)
                [[ $# -ge 2 ]] || die "--app-image requires an image"
                APP_IMAGE="$2"
                APP_IMAGE_OVERRIDE_REQUESTED="true"
                shift 2
                ;;
            --version)
                [[ $# -ge 2 ]] || die "--version requires a tag"
                VERSION="$2"
                VERSION_OVERRIDE_REQUESTED="true"
                shift 2
                ;;
            --skip-start)
                SKIP_START="true"
                shift
                ;;
            -h|--help)
                usage
                exit 0
                ;;
            *)
                die "unsupported option: $1"
                ;;
        esac
    done
}

require_command() {
    command -v "$1" >/dev/null 2>&1 || die "required command not found: $1"
}

install_project_file() {
    local name="$1"
    local destination="$2"
    local mode="$3"
    local script_dir source raw_url

    script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" 2>/dev/null && pwd || pwd)"
    source="${script_dir}/${name}"

    if [[ -f "${source}" ]]; then
        if [[ "$(cd "$(dirname "${source}")" && pwd)/$(basename "${source}")" != "$(cd "$(dirname "${destination}")" && pwd)/$(basename "${destination}")" ]]; then
            install -m "${mode}" "${source}" "${destination}"
        fi
        return
    fi

    require_command curl
    raw_url="https://raw.githubusercontent.com/${REPO}/${SOURCE_REF}/${name}"
    curl -fsSL "${raw_url}" -o "${destination}" ||
        die "failed to download ${raw_url}"
    chmod "${mode}" "${destination}"
}

random_secret() {
    openssl rand -hex 32
}

set_env_value() {
    local file="$1"
    local key="$2"
    local value="$3"
    local temporary

    [[ "${key}" =~ ^[A-Za-z_][A-Za-z0-9_]*$ ]] ||
        die "invalid environment key: ${key}"
    [[ "${value}" != *$'\n'* && "${value}" != *$'\r'* ]] ||
        die "${key} must not contain a newline"

    temporary="$(mktemp "${file}.tmp.XXXXXX")"
    AETHER_INSTALL_ENV_VALUE="${value}" awk -v key="${key}" '
        BEGIN { value = ENVIRON["AETHER_INSTALL_ENV_VALUE"] }
        BEGIN { written = 0 }
        $0 ~ "^[[:space:]#]*" key "=" {
            if (!written) {
                print key "=" value
                written = 1
            }
            next
        }
        { print }
        END {
            if (!written) print key "=" value
        }
    ' "${file}" >"${temporary}"
    chmod 0600 "${temporary}"
    mv "${temporary}" "${file}"
}

read_admin_password() {
    local first second

    if [[ -n "${ADMIN_PASSWORD:-}" ]]; then
        printf '%s' "${ADMIN_PASSWORD}"
        return
    fi

    [[ -r /dev/tty && -w /dev/tty ]] ||
        die "ADMIN_PASSWORD is required for a non-interactive first install"

    while true; do
        printf 'Initial admin password: ' >/dev/tty
        IFS= read -r -s first </dev/tty
        printf '\nConfirm admin password: ' >/dev/tty
        IFS= read -r -s second </dev/tty
        printf '\n' >/dev/tty
        [[ -n "${first}" ]] || {
            warn "password must not be empty"
            continue
        }
        [[ "${first}" == "${second}" ]] || {
            warn "passwords do not match"
            continue
        }
        printf '%s' "${first}"
        return
    done
}

prepare_env() {
    local env_path="${COMPOSE_DIR}/.env"
    local admin_password

    if [[ -f "${env_path}" ]]; then
        warn "keeping existing ${env_path}"
        if [[ "${APP_IMAGE_OVERRIDE_REQUESTED}" == "true" ]]; then
            set_env_value "${env_path}" APP_IMAGE "${APP_IMAGE}"
            info "updated APP_IMAGE in existing ${env_path}"
        elif [[ "${VERSION_OVERRIDE_REQUESTED}" == "true" ]]; then
            set_env_value "${env_path}" APP_IMAGE "ghcr.io/ryfinez/niffler:${VERSION#v}"
            info "updated APP_IMAGE in existing ${env_path}"
        fi
        return
    fi

    if [[ -n "${ENV_SOURCE}" ]]; then
        [[ -f "${ENV_SOURCE}" ]] || die "env file not found: ${ENV_SOURCE}"
        install -m 0600 "${ENV_SOURCE}" "${env_path}"

        if [[ "${APP_IMAGE_OVERRIDE_REQUESTED}" == "true" ]]; then
            set_env_value "${env_path}" APP_IMAGE "${APP_IMAGE}"
        elif [[ "${VERSION_OVERRIDE_REQUESTED}" == "true" ]]; then
            set_env_value "${env_path}" APP_IMAGE "ghcr.io/ryfinez/niffler:${VERSION#v}"
        fi
        return
    else
        install -m 0600 "${COMPOSE_DIR}/.env.example" "${env_path}"
    fi

    require_command openssl
    admin_password="$(read_admin_password)"
    set_env_value "${env_path}" JWT_SECRET_KEY "$(random_secret)"
    set_env_value "${env_path}" ENCRYPTION_KEY "$(random_secret)"
    set_env_value "${env_path}" DB_PASSWORD "$(random_secret)"
    set_env_value "${env_path}" REDIS_PASSWORD "$(random_secret)"
    set_env_value "${env_path}" ADMIN_PASSWORD "${admin_password}"

    if [[ -n "${APP_IMAGE}" ]]; then
        set_env_value "${env_path}" APP_IMAGE "${APP_IMAGE}"
    elif [[ -n "${VERSION}" ]]; then
        set_env_value "${env_path}" APP_IMAGE "ghcr.io/ryfinez/niffler:${VERSION#v}"
    fi
}

start_deployment() {
    require_command docker
    docker compose version >/dev/null 2>&1 ||
        die "Docker Compose v2 is required"
    (
        cd "${COMPOSE_DIR}"
        docker compose pull
        docker compose up -d
    )
}

main() {
    parse_args "$@"

    case "${MODE}" in
        compose|postgres)
            ;;
        *)
            die "unsupported install mode: ${MODE}; only PostgreSQL Docker Compose is available"
            ;;
    esac

    mkdir -p "${COMPOSE_DIR}" "${COMPOSE_DIR}/logs"
    COMPOSE_DIR="$(cd "${COMPOSE_DIR}" && pwd)"

    info "preparing PostgreSQL Docker Compose deployment in ${COMPOSE_DIR}"
    install_project_file docker-compose.yml "${COMPOSE_DIR}/docker-compose.yml" 0644
    install_project_file .env.example "${COMPOSE_DIR}/.env.example" 0644
    install_project_file generate_keys.sh "${COMPOSE_DIR}/generate_keys.sh" 0755
    prepare_env

    if [[ "${SKIP_START}" == "true" ]]; then
        echo "Files are ready in ${COMPOSE_DIR}. Start with: docker compose up -d"
        return
    fi

    start_deployment
    echo "Niffler is running. Check status with: cd ${COMPOSE_DIR} && docker compose ps"
}

main "$@"
