#!/bin/bash

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
DEPLOYER="$SCRIPT_DIR/../fixed-production-deployer.sh"
TEST_ROOT="$(mktemp -d)"
trap 'rm -rf "$TEST_ROOT"' EXIT

CURRENT_COMMIT="aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
TARGET_COMMIT="bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
OTHER_COMMIT="cccccccccccccccccccccccccccccccccccccccc"
FAKE_BIN="$TEST_ROOT/bin"
REMOTE_DIR="$TEST_ROOT/app"
RELEASE_ROOT="$TEST_ROOT/release"
DOCKER_LOG="$TEST_ROOT/docker.log"
mkdir -p "$FAKE_BIN"

cat > "$FAKE_BIN/git" <<'FAKE_GIT'
#!/bin/bash
set -euo pipefail
if [ "${1:-}" = "ls-remote" ]; then
    printf '%s\trefs/heads/main\n' "$FAKE_MAIN_COMMIT"
    exit 0
fi
if [ "${1:-}" = "init" ] && [ "${2:-}" = "--bare" ]; then
    mkdir -p "$3"
    exit 0
fi
if [[ "${1:-}" == --git-dir=* ]]; then
    shift
    case "${1:-}" in
        fetch|cat-file|merge-base)
            exit 0
            ;;
    esac
fi
echo "unexpected fake git invocation: $*" >&2
exit 2
FAKE_GIT

cat > "$FAKE_BIN/docker" <<'FAKE_DOCKER'
#!/bin/bash
set -euo pipefail
printf '%s\n' "$*" >> "$FAKE_DOCKER_LOG"
printf 'env APP_IMAGE=%s command=%s\n' "${APP_IMAGE:-}" "$*" >> "$FAKE_DOCKER_LOG"
case "${1:-}" in
    compose)
        shift
        case "${1:-}" in
            version|up|run)
                exit 0
                ;;
            ps)
                if [ "${2:-}" = "-q" ]; then
                    if [ "${FAKE_CONTEXT_RUNNING:-true}" = "false" ]; then
                        exit 0
                    fi
                    printf '%s-container\n' "${3:-unknown}"
                fi
                exit 0
                ;;
        esac
        ;;
    image)
        if [ "${2:-}" = "inspect" ]; then
            if [ "${FAKE_HAS_CURRENT_IMAGE:-true}" = "false" ] \
                && [[ "$*" == *"niffler-app:latest"* ]]; then
                exit 1
            fi
            if [[ "$*" == *"--format"* ]]; then
                printf '%s\n' "$FAKE_TARGET_COMMIT"
            fi
            exit 0
        fi
        if [ "${2:-}" = "tag" ]; then
            exit 0
        fi
        ;;
    load)
        exit 0
        ;;
    inspect)
        if [[ "${2:-}" == *-container ]]; then
            printf 'AETHER_DATABASE_DRIVER=postgres\n'
            printf 'AETHER_DATABASE_URL=postgres://example.invalid/aether\n'
            exit 0
        fi
        ;;
    run)
        if [ "${FAKE_MIGRATION_COMPATIBLE:-true}" = "true" ]; then
            printf 'PostgreSQL migration compatibility verified; pending migrations: 0\n'
            exit 0
        fi
        echo 'VersionMissing(20260723121000)' >&2
        exit 1
        ;;
esac
echo "unexpected fake docker invocation: $*" >&2
exit 2
FAKE_DOCKER

cat > "$FAKE_BIN/curl" <<'FAKE_CURL'
#!/bin/bash
set -euo pipefail
if [ "${FAKE_CURL_FAIL:-false}" = "true" ]; then
    exit 22
fi
printf '{"status":"ok"}\n'
FAKE_CURL

chmod +x "$FAKE_BIN/git" "$FAKE_BIN/docker" "$FAKE_BIN/curl"

export PATH="$FAKE_BIN:$PATH"
export REPO_URL="https://example.invalid/Niffler.git"
export RELEASE_ROOT
export FAKE_MAIN_COMMIT="$TARGET_COMMIT"
export FAKE_TARGET_COMMIT="$TARGET_COMMIT"
export FAKE_DOCKER_LOG="$DOCKER_LOG"
export FAKE_CURL_FAIL=false
export FAKE_MIGRATION_COMPATIBLE=true
export FAKE_HAS_CURRENT_IMAGE=true
export FAKE_CONTEXT_RUNNING=true

reset_fixture() {
    rm -rf "$REMOTE_DIR" "$RELEASE_ROOT"
    mkdir -p "$REMOTE_DIR"
    : > "$REMOTE_DIR/docker-compose.yml"
    printf 'APP_IMAGE=niffler-app:latest\n' > "$REMOTE_DIR/.env"
    printf '%s\n' "$CURRENT_COMMIT" > "$REMOTE_DIR/.niffler-deployed-commit"
    : > "$REMOTE_DIR/image.tar"
    : > "$DOCKER_LOG"
    export FAKE_HAS_CURRENT_IMAGE=true
    export FAKE_CONTEXT_RUNNING=true
}

reset_first_test_fixture() {
    rm -rf "$REMOTE_DIR" "$RELEASE_ROOT"
    mkdir -p "$REMOTE_DIR"
    : > "$REMOTE_DIR/docker-compose.yml"
    printf 'APP_IMAGE=ghcr.io/example/old-image\n' > "$REMOTE_DIR/.env"
    : > "$REMOTE_DIR/image.tar"
    : > "$DOCKER_LOG"
    export FAKE_HAS_CURRENT_IMAGE=false
    export FAKE_CONTEXT_RUNNING=false
}

run_deployer() {
    bash "$DEPLOYER" \
        --image-tar "$REMOTE_DIR/image.tar" \
        --target "$1" \
        --remote-dir "$REMOTE_DIR" \
        --source-health-url "http://source.test/health" \
        --public-health-url "https://public.test/health"
}

run_test_deployer() {
    bash "$DEPLOYER" \
        --image-tar "$REMOTE_DIR/image.tar" \
        --target "$1" \
        --remote-dir "$REMOTE_DIR" \
        --service app \
        --required-branch test \
        --migration-context-service app \
        --allow-non-ancestor-current \
        --source-health-url "http://source.test/health" \
        --public-health-url "https://public.test/health"
}

run_first_test_deployer() {
    bash "$DEPLOYER" \
        --image-tar "$REMOTE_DIR/image.tar" \
        --target "$1" \
        --remote-dir "$REMOTE_DIR" \
        --service app \
        --required-branch test \
        --migration-context-service app \
        --bootstrap-migration-context \
        --allow-non-ancestor-current \
        --source-health-url "http://source.test/health" \
        --public-health-url "https://public.test/health"
}

assert_contains() {
    local file="$1"
    local expected="$2"
    grep -Fq -- "$expected" "$file" || {
        echo "expected '$expected' in $file" >&2
        exit 1
    }
}

assert_not_contains() {
    local file="$1"
    local unexpected="$2"
    if grep -Fq -- "$unexpected" "$file"; then
        echo "did not expect '$unexpected' in $file" >&2
        exit 1
    fi
}

reset_fixture
if run_deployer "$OTHER_COMMIT" >"$TEST_ROOT/not-main.out" 2>&1; then
    echo "non-main deployment unexpectedly succeeded" >&2
    exit 1
fi
assert_contains "$TEST_ROOT/not-main.out" "is not current origin/main"
assert_not_contains "$DOCKER_LOG" "load -i"

reset_fixture
export FAKE_MIGRATION_COMPATIBLE=false
if run_deployer "$TARGET_COMMIT" >"$TEST_ROOT/missing-migration.out" 2>&1; then
    echo "missing-migration deployment unexpectedly succeeded" >&2
    exit 1
fi
assert_contains "$TEST_ROOT/missing-migration.out" "incompatible with the active PostgreSQL migration history"
assert_not_contains "$DOCKER_LOG" "compose up"
test "$(cat "$REMOTE_DIR/.niffler-deployed-commit")" = "$CURRENT_COMMIT"

reset_fixture
export FAKE_MIGRATION_COMPATIBLE=true
export FAKE_CURL_FAIL=true
if run_deployer "$TARGET_COMMIT" >"$TEST_ROOT/health-failure.out" 2>&1; then
    echo "health-failure deployment unexpectedly succeeded" >&2
    exit 1
fi
assert_contains "$TEST_ROOT/health-failure.out" "restoring niffler-app:rollback-"
test "$(grep -c '^compose up ' "$DOCKER_LOG")" -eq 2
test "$(cat "$REMOTE_DIR/.niffler-deployed-commit")" = "$CURRENT_COMMIT"
assert_contains "$REMOTE_DIR/.env" "APP_IMAGE=niffler-app:rollback-"
assert_contains "$DOCKER_LOG" "image tag niffler-app:rollback-"

reset_fixture
export FAKE_CURL_FAIL=false
run_deployer "$TARGET_COMMIT" >"$TEST_ROOT/success.out" 2>&1
assert_contains "$TEST_ROOT/success.out" "Deployment verified for origin/main"
assert_contains "$DOCKER_LOG" "inspect frontdoor-container --format"
assert_contains "$DOCKER_LOG" "run --rm --network container:frontdoor-container --volumes-from frontdoor-container:ro --env-file"
assert_contains "$DOCKER_LOG" "--check-postgres-migration-compatibility"
test "$(cat "$REMOTE_DIR/.niffler-deployed-commit")" = "$TARGET_COMMIT"
test ! -e "$REMOTE_DIR/image.tar"
assert_contains "$REMOTE_DIR/.env" "APP_IMAGE=niffler-app:$TARGET_COMMIT"

reset_fixture
export FAKE_CURL_FAIL=false
run_test_deployer "$TARGET_COMMIT" >"$TEST_ROOT/test-success.out" 2>&1
assert_contains "$TEST_ROOT/test-success.out" "Deployment verified for origin/test"
assert_contains "$DOCKER_LOG" "compose ps -q app"
assert_contains "$DOCKER_LOG" "inspect app-container --format"
assert_contains "$DOCKER_LOG" "--network container:app-container"
assert_contains "$DOCKER_LOG" "--check-postgres-migration-compatibility"
assert_contains "$DOCKER_LOG" "compose up"

reset_first_test_fixture
export FAKE_CURL_FAIL=false
if run_first_test_deployer "$TARGET_COMMIT" >"$TEST_ROOT/first-test-no-marker.out" 2>&1; then
    echo "first test deployment without environment marker unexpectedly succeeded" >&2
    exit 1
fi
assert_contains "$TEST_ROOT/first-test-no-marker.out" "test environment marker is missing"
assert_not_contains "$DOCKER_LOG" "compose up"

printf 'niffler-test-v1\n' > "$REMOTE_DIR/.niffler-test-environment"
run_first_test_deployer "$TARGET_COMMIT" >"$TEST_ROOT/first-test-success.out" 2>&1
assert_contains "$TEST_ROOT/first-test-success.out" "preparing first test deployment"
assert_contains "$TEST_ROOT/first-test-success.out" "Deployment verified for origin/test"
assert_contains "$DOCKER_LOG" "compose up -d --no-build postgres redis"
assert_contains "$DOCKER_LOG" "compose run --rm --no-deps --entrypoint /usr/local/bin/aether-gateway app --check-postgres-migration-compatibility"
assert_contains "$DOCKER_LOG" "env APP_IMAGE=niffler-app:$TARGET_COMMIT command=compose run"
assert_contains "$DOCKER_LOG" "compose up -d --no-build --force-recreate --wait --wait-timeout"

echo "fixed production deployer tests passed"
