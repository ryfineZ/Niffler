#!/bin/bash

set -euo pipefail

REPO_URL="${REPO_URL:-https://github.com/ryfineZ/Niffler.git}"
RELEASE_ROOT="${RELEASE_ROOT:-/opt/niffler-release}"
REQUIRED_BRANCH="main"
REMOTE_DIR="/opt/niffler-app"
IMAGE_TAR=""
TARGET_COMMIT=""
DEPLOY_STATE_FILE=".niffler-deployed-commit"
APP_IMAGE_REPOSITORY="niffler-app"
PUBLIC_HEALTH_URL="https://niffler.org/_gateway/health"
SOURCE_HEALTH_URL="http://127.0.0.1:8084/_gateway/health"
HEALTH_TIMEOUT_SECONDS=180
ALLOW_ROLLBACK=false
ENFORCE_CURRENT_ANCESTRY=true
MIGRATION_CONTEXT_SERVICE="frontdoor"
BOOTSTRAP_MIGRATION_CONTEXT=false
TEST_ENVIRONMENT_MARKER=".niffler-test-environment"
SERVICES=()

usage() {
    cat <<'EOF'
Usage: fixed-production-deployer.sh --image-tar <path> --target <sha> [options]

Options:
  --image-tar <path>            Image tar produced by Build App Image
  --target <sha>                Exact 40-character commit to deploy
  --remote-dir <path>           Compose directory, default /opt/niffler-app
  --service <name>              Compose service to recreate; repeat as needed
  --state-file <name>           Deployed commit state file name
  --required-branch <branch>    Exact remote branch the target must match, default main
  --migration-context-service <name>
                                Compose service whose network and environment reach PostgreSQL
  --bootstrap-migration-context
                                For a first test deployment, start PostgreSQL and Redis and run
                                the migration check from the target Compose service when no active
                                migration-context container exists
  --allow-non-ancestor-current Allow replacing an older test lineage while still requiring
                                the exact current remote branch commit
  --public-health-url <url>     Public health endpoint
  --source-health-url <url>     Source health endpoint
  --health-timeout <seconds>    Compose health wait timeout, default 180
  --allow-rollback              Permit a target other than current origin/main
  -h, --help                    Show help

Environment:
  REPO_URL                      Public Git repository used to resolve main
  RELEASE_ROOT                  Fixed release state root
EOF
}

die() {
    echo "ERROR: $*" >&2
    exit 1
}

require_option_value() {
    local option_name="$1"
    local option_value="${2:-}"
    if [ -z "$option_value" ] || [[ "$option_value" == --* ]]; then
        die "missing value for $option_name"
    fi
}

while [ $# -gt 0 ]; do
    case "$1" in
        --image-tar)
            require_option_value "$1" "${2:-}"
            IMAGE_TAR="$2"
            shift 2
            ;;
        --target)
            require_option_value "$1" "${2:-}"
            TARGET_COMMIT="$2"
            shift 2
            ;;
        --remote-dir)
            require_option_value "$1" "${2:-}"
            REMOTE_DIR="$2"
            shift 2
            ;;
        --service)
            require_option_value "$1" "${2:-}"
            SERVICES+=("$2")
            shift 2
            ;;
        --state-file)
            require_option_value "$1" "${2:-}"
            DEPLOY_STATE_FILE="$2"
            shift 2
            ;;
        --required-branch)
            require_option_value "$1" "${2:-}"
            REQUIRED_BRANCH="$2"
            shift 2
            ;;
        --migration-context-service)
            require_option_value "$1" "${2:-}"
            MIGRATION_CONTEXT_SERVICE="$2"
            shift 2
            ;;
        --bootstrap-migration-context)
            BOOTSTRAP_MIGRATION_CONTEXT=true
            shift
            ;;
        --allow-non-ancestor-current)
            ENFORCE_CURRENT_ANCESTRY=false
            shift
            ;;
        --public-health-url)
            require_option_value "$1" "${2:-}"
            PUBLIC_HEALTH_URL="$2"
            shift 2
            ;;
        --source-health-url)
            require_option_value "$1" "${2:-}"
            SOURCE_HEALTH_URL="$2"
            shift 2
            ;;
        --health-timeout)
            require_option_value "$1" "${2:-}"
            HEALTH_TIMEOUT_SECONDS="$2"
            shift 2
            ;;
        --allow-rollback)
            ALLOW_ROLLBACK=true
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            die "unknown argument: $1"
            ;;
    esac
done

if [ -z "$IMAGE_TAR" ] || [ -z "$TARGET_COMMIT" ]; then
    usage >&2
    die "--image-tar and --target are required"
fi
if [[ ! "$TARGET_COMMIT" =~ ^[0-9a-f]{40}$ ]]; then
    die "target commit must be a lowercase 40-character SHA"
fi
if [[ ! "$REQUIRED_BRANCH" =~ ^[A-Za-z0-9._/-]+$ ]] \
    || [[ "$REQUIRED_BRANCH" == /* ]] \
    || [[ "$REQUIRED_BRANCH" == */ ]] \
    || [[ "$REQUIRED_BRANCH" == *..* ]]; then
    die "required branch is invalid"
fi
if [[ ! "$HEALTH_TIMEOUT_SECONDS" =~ ^[1-9][0-9]*$ ]]; then
    die "health timeout must be a positive integer"
fi
if [ "${#SERVICES[@]}" -eq 0 ]; then
    SERVICES=(frontdoor background)
fi
for service in "${SERVICES[@]}"; do
    if [[ ! "$service" =~ ^[a-zA-Z0-9][a-zA-Z0-9_-]*$ ]]; then
        die "invalid compose service name: $service"
    fi
done
if [[ ! "$MIGRATION_CONTEXT_SERVICE" =~ ^[a-zA-Z0-9][a-zA-Z0-9_-]*$ ]]; then
    die "invalid migration context service name"
fi
if [ "$BOOTSTRAP_MIGRATION_CONTEXT" = true ] && [ "$REQUIRED_BRANCH" != "test" ]; then
    die "bootstrap migration context is only allowed for the test branch"
fi
if [[ "$REMOTE_DIR" != /* ]] || [ "$REMOTE_DIR" = "/" ]; then
    die "remote directory must be an explicit absolute application directory"
fi
if [ "$BOOTSTRAP_MIGRATION_CONTEXT" = true ]; then
    TEST_MARKER_PATH="$REMOTE_DIR/$TEST_ENVIRONMENT_MARKER"
    if [ ! -f "$TEST_MARKER_PATH" ] || [ -L "$TEST_MARKER_PATH" ]; then
        die "test environment marker is missing: $TEST_MARKER_PATH"
    fi
    if [ "$(tr -d '\r\n' < "$TEST_MARKER_PATH")" != "niffler-test-v1" ]; then
        die "test environment marker is invalid: $TEST_MARKER_PATH"
    fi
fi
if [[ "$RELEASE_ROOT" != /* ]] || [ "$RELEASE_ROOT" = "/" ]; then
    die "release root must be an explicit absolute directory"
fi
if [[ "$DEPLOY_STATE_FILE" == */* ]] || [ -z "$DEPLOY_STATE_FILE" ]; then
    die "state file must be a file name inside the application directory"
fi
if [ ! -f "$IMAGE_TAR" ]; then
    die "image tar not found: $IMAGE_TAR"
fi
if [ ! -f "$REMOTE_DIR/docker-compose.yml" ]; then
    die "compose file not found: $REMOTE_DIR/docker-compose.yml"
fi

for command_name in git docker curl awk grep mktemp install cp mv date sed tr chmod; do
    command -v "$command_name" >/dev/null 2>&1 || die "required command not found: $command_name"
done

if docker compose version >/dev/null 2>&1; then
    DC=(docker compose)
elif command -v docker-compose >/dev/null 2>&1; then
    DC=(docker-compose)
else
    die "docker compose is not installed"
fi

mkdir -p "$RELEASE_ROOT/git" "$RELEASE_ROOT/state"
RELEASE_REPO="$RELEASE_ROOT/git/Niffler.git"
if [ ! -d "$RELEASE_REPO" ]; then
    git init --bare "$RELEASE_REPO" >/dev/null
fi

REMOTE_REQUIRED_COMMIT="$(
    git ls-remote "$REPO_URL" "refs/heads/$REQUIRED_BRANCH" \
        | awk 'NR == 1 { print $1 }'
)"
if [[ ! "$REMOTE_REQUIRED_COMMIT" =~ ^[0-9a-f]{40}$ ]]; then
    die "unable to resolve origin/$REQUIRED_BRANCH from $REPO_URL"
fi
if [ "$ALLOW_ROLLBACK" != true ] && [ "$TARGET_COMMIT" != "$REMOTE_REQUIRED_COMMIT" ]; then
    die "normal deployment target $TARGET_COMMIT is not current origin/$REQUIRED_BRANCH $REMOTE_REQUIRED_COMMIT"
fi

git --git-dir="$RELEASE_REPO" fetch --quiet --force "$REPO_URL" \
    "+refs/heads/$REQUIRED_BRANCH:refs/heads/$REQUIRED_BRANCH" \
    "+refs/tags/archive/*:refs/tags/archive/*"
if ! git --git-dir="$RELEASE_REPO" cat-file -e "${TARGET_COMMIT}^{commit}" 2>/dev/null; then
    die "target commit is not reachable from origin/$REQUIRED_BRANCH or an archive tag: $TARGET_COMMIT"
fi

STATE_PATH="$REMOTE_DIR/$DEPLOY_STATE_FILE"
CURRENT_COMMIT=""
if [ -s "$STATE_PATH" ]; then
    CURRENT_COMMIT="$(tr -d '[:space:]' < "$STATE_PATH")"
    if [[ ! "$CURRENT_COMMIT" =~ ^[0-9a-f]{40}$ ]]; then
        die "invalid deployed commit state: $STATE_PATH"
    fi
    if [ "$ALLOW_ROLLBACK" != true ] && [ "$ENFORCE_CURRENT_ANCESTRY" = true ]; then
        if ! git --git-dir="$RELEASE_REPO" cat-file -e "${CURRENT_COMMIT}^{commit}" 2>/dev/null; then
            die "current production commit is not available in the fixed release repository"
        fi
        if ! git --git-dir="$RELEASE_REPO" merge-base --is-ancestor \
            "$CURRENT_COMMIT" "$TARGET_COMMIT"; then
            die "target does not contain current production commit $CURRENT_COMMIT"
        fi
    fi
fi

if [ -z "$CURRENT_COMMIT" ] \
    && docker image inspect "$APP_IMAGE_REPOSITORY:latest" >/dev/null 2>&1; then
    CURRENT_COMMIT="$(
        docker image inspect "$APP_IMAGE_REPOSITORY:latest" \
            --format '{{ index .Config.Labels "org.opencontainers.image.revision" }}' \
            2>/dev/null || true
    )"
    if [[ ! "$CURRENT_COMMIT" =~ ^[0-9a-f]{40}$ ]]; then
        CURRENT_COMMIT="$(
            docker image inspect "$APP_IMAGE_REPOSITORY:latest" \
                --format '{{range .RepoTags}}{{println .}}{{end}}' \
                2>/dev/null \
                | awk -F: '$NF ~ /^[0-9a-f]{40}$/ { print $NF; exit }'
        )"
    fi
    if [[ ! "$CURRENT_COMMIT" =~ ^[0-9a-f]{40}$ ]]; then
        CURRENT_COMMIT=""
    elif [ "$ALLOW_ROLLBACK" != true ] && [ "$ENFORCE_CURRENT_ANCESTRY" = true ]; then
        if ! git --git-dir="$RELEASE_REPO" cat-file -e "${CURRENT_COMMIT}^{commit}" 2>/dev/null; then
            die "current deployed image commit is not available in the fixed release repository"
        fi
        if ! git --git-dir="$RELEASE_REPO" merge-base --is-ancestor \
            "$CURRENT_COMMIT" "$TARGET_COMMIT"; then
            die "target does not contain current deployed image commit $CURRENT_COMMIT"
        fi
    fi
fi

TIMESTAMP="$(date -u +%Y%m%dT%H%M%SZ)"
TARGET_IMAGE="$APP_IMAGE_REPOSITORY:$TARGET_COMMIT"
OLD_IMAGE=""
ROLLBACK_IMAGE=""
if [ -n "$CURRENT_COMMIT" ] \
    && docker image inspect "$APP_IMAGE_REPOSITORY:$CURRENT_COMMIT" >/dev/null 2>&1; then
    OLD_IMAGE="$APP_IMAGE_REPOSITORY:$CURRENT_COMMIT"
elif docker image inspect "$APP_IMAGE_REPOSITORY:latest" >/dev/null 2>&1; then
    OLD_IMAGE="$APP_IMAGE_REPOSITORY:latest"
fi
if [ -n "$OLD_IMAGE" ]; then
    ROLLBACK_IMAGE="$APP_IMAGE_REPOSITORY:rollback-${CURRENT_COMMIT:0:12}-$TIMESTAMP"
    docker image tag "$OLD_IMAGE" "$ROLLBACK_IMAGE"
fi
if [ -z "$CURRENT_COMMIT" ] && [ -n "$OLD_IMAGE" ] && [ "$ALLOW_ROLLBACK" != true ]; then
    die "an application image exists but the deployed commit state is missing"
fi

docker load -i "$IMAGE_TAR"
if [ -n "$ROLLBACK_IMAGE" ]; then
    docker image tag "$ROLLBACK_IMAGE" "$APP_IMAGE_REPOSITORY:latest"
fi
if ! docker image inspect "$TARGET_IMAGE" >/dev/null 2>&1; then
    die "loaded artifact does not contain exact image tag $TARGET_IMAGE"
fi
IMAGE_REVISION="$(
    docker image inspect "$TARGET_IMAGE" \
        --format '{{ index .Config.Labels "org.opencontainers.image.revision" }}'
)"
if [ "$IMAGE_REVISION" != "$TARGET_COMMIT" ]; then
    die "image revision $IMAGE_REVISION does not match target $TARGET_COMMIT"
fi

TMP_DIR="$(mktemp -d "$RELEASE_ROOT/state/preflight.XXXXXX")"
cleanup() {
    rm -rf "$TMP_DIR"
}
trap cleanup EXIT

cd "$REMOTE_DIR"
MIGRATION_CONTEXT_CONTAINER="$("${DC[@]}" ps -q "$MIGRATION_CONTEXT_SERVICE")"
if [ -z "$MIGRATION_CONTEXT_CONTAINER" ]; then
    if [ "$BOOTSTRAP_MIGRATION_CONTEXT" != true ]; then
        die "$MIGRATION_CONTEXT_SERVICE container is not running; cannot read the active PostgreSQL environment"
    fi
    if [ -n "$CURRENT_COMMIT" ]; then
        die "$MIGRATION_CONTEXT_SERVICE container is not running for an existing deployment"
    fi
    echo "No active $MIGRATION_CONTEXT_SERVICE container; preparing first test deployment."
    if ! "${DC[@]}" up -d --no-build postgres redis; then
        die "failed to start PostgreSQL and Redis for the first test deployment"
    fi
    if ! APP_IMAGE="$TARGET_IMAGE" "${DC[@]}" run --rm --no-deps \
        --entrypoint /usr/local/bin/aether-gateway \
        "$MIGRATION_CONTEXT_SERVICE" \
        --check-postgres-migration-compatibility; then
        die "target image is incompatible with the test PostgreSQL migration history"
    fi
else
    CURRENT_ENV_FILE="$TMP_DIR/$MIGRATION_CONTEXT_SERVICE.env"
    docker inspect "$MIGRATION_CONTEXT_CONTAINER" \
        --format '{{range .Config.Env}}{{println .}}{{end}}' > "$CURRENT_ENV_FILE"
    chmod 0600 "$CURRENT_ENV_FILE"
    if ! docker run --rm \
        --network "container:$MIGRATION_CONTEXT_CONTAINER" \
        --volumes-from "$MIGRATION_CONTEXT_CONTAINER:ro" \
        --env-file "$CURRENT_ENV_FILE" \
        --entrypoint /usr/local/bin/aether-gateway \
        "$TARGET_IMAGE" \
        --check-postgres-migration-compatibility; then
        die "target image is incompatible with the active PostgreSQL migration history"
    fi
fi

ENV_PATH="$REMOTE_DIR/.env"
ENV_BACKUP="$RELEASE_ROOT/state/env-$TIMESTAMP"
if [ -f "$ENV_PATH" ]; then
    cp "$ENV_PATH" "$ENV_BACKUP"
else
    : > "$ENV_BACKUP"
fi

set_app_image() {
    local image_ref="$1"
    if [ -f "$ENV_PATH" ] && grep -q '^APP_IMAGE=' "$ENV_PATH"; then
        sed -i.bak "s|^APP_IMAGE=.*|APP_IMAGE=$image_ref|" "$ENV_PATH"
        rm -f "$ENV_PATH.bak"
    else
        printf '\nAPP_IMAGE=%s\n' "$image_ref" >> "$ENV_PATH"
    fi
}

rollback() {
    local failed_status="$1"
    if [ -z "$ROLLBACK_IMAGE" ]; then
        echo "No previous image is available for automatic rollback." >&2
        return "$failed_status"
    fi

    echo "Deployment failed; restoring $ROLLBACK_IMAGE..." >&2
    docker image tag "$TARGET_IMAGE" \
        "$APP_IMAGE_REPOSITORY:failed-${TARGET_COMMIT:0:12}-$TIMESTAMP"
    docker image tag "$ROLLBACK_IMAGE" "$APP_IMAGE_REPOSITORY:latest"
    set_app_image "$ROLLBACK_IMAGE"
    if ! "${DC[@]}" up -d --no-build --force-recreate \
        --wait --wait-timeout "$HEALTH_TIMEOUT_SECONDS" "${SERVICES[@]}"; then
        echo "Automatic rollback did not become healthy." >&2
        "${DC[@]}" ps >&2 || true
        return "$failed_status"
    fi
    "${DC[@]}" ps
    return "$failed_status"
}

set_app_image "$TARGET_IMAGE"
if ! "${DC[@]}" up -d --no-build --force-recreate \
    --wait --wait-timeout "$HEALTH_TIMEOUT_SECONDS" "${SERVICES[@]}"; then
    rollback 1
    exit $?
fi
if ! curl -fsS --max-time 15 "$SOURCE_HEALTH_URL" >/dev/null; then
    echo "Source health check failed: $SOURCE_HEALTH_URL" >&2
    rollback 1
    exit $?
fi
if ! curl -fsS --max-time 20 "$PUBLIC_HEALTH_URL" >/dev/null; then
    echo "Public health check failed: $PUBLIC_HEALTH_URL" >&2
    rollback 1
    exit $?
fi

docker image tag "$TARGET_IMAGE" "$APP_IMAGE_REPOSITORY:latest"
STATE_TMP="$RELEASE_ROOT/state/deployed-commit.tmp"
printf '%s\n' "$TARGET_COMMIT" > "$STATE_TMP"
install -m 0644 "$STATE_TMP" "$STATE_PATH"
rm -f "$STATE_TMP" "$IMAGE_TAR"
"${DC[@]}" ps
echo "Deployment verified for origin/$REQUIRED_BRANCH: $TARGET_COMMIT"
