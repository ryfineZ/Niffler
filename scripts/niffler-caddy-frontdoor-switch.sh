#!/usr/bin/env bash
set -Eeuo pipefail

MODE="${1:?mode must be hd0526 or ovh}"
TARGET="${2:-next}"
CADDYFILE="${NIFFLER_CADDYFILE:-/opt/niffler-app/Caddyfile}"
CADDY_CONTAINER="${NIFFLER_CADDY_CONTAINER:-niffler-caddy}"
TIMESTAMP="$(date -u +%Y%m%dT%H%M%SZ)"
BACKUP_DIR="/root/niffler-caddy-pgbouncer-${TIMESTAMP}"
HOST_BEFORE="${BACKUP_DIR}/Caddyfile.host.before"
ACTIVE_BEFORE="${BACKUP_DIR}/Caddyfile.active.before"
HOST_AFTER="${BACKUP_DIR}/Caddyfile.host.after"
ACTIVE_AFTER="${BACKUP_DIR}/Caddyfile.active.after"
SWITCHED=false
SYNC_ONLY=false
STAGE=preflight

case "$MODE:$TARGET" in
    hd0526:next)
        OLD_UPSTREAM="niffler-frontdoor:8084"
        NEW_UPSTREAM="niffler-frontdoor-next:8084"
        EXPECTED_COUNT=3
        ;;
    hd0526:stable)
        OLD_UPSTREAM="niffler-frontdoor-next:8084"
        NEW_UPSTREAM="niffler-frontdoor:8084"
        EXPECTED_COUNT=3
        ;;
    hd0526:sync)
        CURRENT_UPSTREAM="niffler-frontdoor:8084"
        STALE_UPSTREAM="niffler-frontdoor-next:8084"
        EXPECTED_COUNT=3
        SYNC_ONLY=true
        ;;
    ovh:next)
        OLD_UPSTREAM="127.0.0.1:18084"
        NEW_UPSTREAM="127.0.0.1:18086"
        EXPECTED_COUNT=2
        ;;
    ovh:stable)
        OLD_UPSTREAM="127.0.0.1:18086"
        NEW_UPSTREAM="127.0.0.1:18084"
        EXPECTED_COUNT=2
        ;;
    ovh:sync)
        CURRENT_UPSTREAM="127.0.0.1:18084"
        STALE_UPSTREAM="127.0.0.1:18086"
        EXPECTED_COUNT=2
        SYNC_ONLY=true
        ;;
    *)
        printf 'unsupported mode or target: mode=%s target=%s\n' "$MODE" "$TARGET" >&2
        exit 2
        ;;
esac

write_regular_file() {
    local source_file="$1"
    local target_file="$2"
    python3 - "$source_file" "$target_file" <<'PY'
import os
import sys

source_path, target_path = sys.argv[1:]
with open(source_path, "rb") as source:
    content = source.read()
with open(target_path, "r+b") as target:
    target.seek(0)
    target.write(content)
    target.truncate()
    target.flush()
    os.fsync(target.fileno())
PY
}

write_active_file() {
    local source_file="$1"
    local caddy_pid
    caddy_pid="$(docker inspect --format '{{.State.Pid}}' "$CADDY_CONTAINER")"
    nsenter -t "$caddy_pid" -m -r -- \
        mount -o remount,rw,bind /etc/caddy/Caddyfile /etc/caddy/Caddyfile
    if ! cat "$source_file" | nsenter -t "$caddy_pid" -m -r -- \
        sh -c 'cat > /etc/caddy/Caddyfile && sync'; then
        nsenter -t "$caddy_pid" -m -r -- \
            mount -o remount,ro,bind /etc/caddy/Caddyfile /etc/caddy/Caddyfile || true
        return 1
    fi
    nsenter -t "$caddy_pid" -m -r -- \
        mount -o remount,ro,bind /etc/caddy/Caddyfile /etc/caddy/Caddyfile
}

build_candidate() {
    local source_file="$1"
    local target_file="$2"
    python3 - "$source_file" "$target_file" "$OLD_UPSTREAM" "$NEW_UPSTREAM" "$EXPECTED_COUNT" <<'PY'
import os
import sys

source_path, target_path, old, new, expected_text = sys.argv[1:]
expected = int(expected_text)
with open(source_path, encoding="utf-8") as source:
    content = source.read()
old_count = content.count(old)
new_count = content.count(new)
if old_count != expected or new_count != 0:
    raise SystemExit(f"unexpected upstream counts: old={old_count} new={new_count} expected={expected}")
updated = content.replace(old, new)
descriptor = os.open(target_path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
with os.fdopen(descriptor, "w", encoding="utf-8") as target:
    target.write(updated)
    target.flush()
    os.fsync(target.fileno())
PY
}

rollback_on_error() {
    local exit_code=$?
    if [ "$exit_code" -eq 0 ]; then
        return
    fi
    trap - ERR
    set +e
    if [ "$SWITCHED" = true ] && [ -f "$HOST_BEFORE" ] && [ -f "$ACTIVE_BEFORE" ]; then
        write_regular_file "$HOST_BEFORE" "$CADDYFILE"
        write_active_file "$ACTIVE_BEFORE"
        docker exec "$CADDY_CONTAINER" caddy validate --config /etc/caddy/Caddyfile --adapter caddyfile >/dev/null 2>&1
        docker exec "$CADDY_CONTAINER" caddy reload --config /etc/caddy/Caddyfile --adapter caddyfile >/dev/null 2>&1 || true
    fi
    printf 'Caddy switch failed at stage=%s; rollback attempted from %s\n' "$STAGE" "$BACKUP_DIR" >&2
    exit "$exit_code"
}
trap rollback_on_error ERR

test -f "$CADDYFILE"
test ! -L "$CADDYFILE"
install -d -m 0700 "$BACKUP_DIR"
install -m 0600 "$CADDYFILE" "$HOST_BEFORE"
docker exec "$CADDY_CONTAINER" cat /etc/caddy/Caddyfile >"$ACTIVE_BEFORE"
chmod 0600 "$ACTIVE_BEFORE"

if [ "$SYNC_ONLY" = true ]; then
    STAGE=verify_host_source
    test "$(grep -Fc "$CURRENT_UPSTREAM" "$HOST_BEFORE")" = "$EXPECTED_COUNT"
    test "$(grep -Fc "$STALE_UPSTREAM" "$HOST_BEFORE" || true)" = 0
    install -m 0600 "$HOST_BEFORE" "$ACTIVE_AFTER"

    STAGE=write_active_mount
    SWITCHED=true
    write_active_file "$ACTIVE_AFTER"

    STAGE=validate
    docker exec "$CADDY_CONTAINER" caddy validate --config /etc/caddy/Caddyfile --adapter caddyfile >/dev/null

    STAGE=reload
    docker exec "$CADDY_CONTAINER" caddy reload --config /etc/caddy/Caddyfile --adapter caddyfile >/dev/null

    STAGE=verify_identical
    test "$(sha256sum "$CADDYFILE" | awk '{print $1}')" = \
        "$(docker exec "$CADDY_CONTAINER" sha256sum /etc/caddy/Caddyfile | awk '{print $1}')"

    trap - ERR
    printf 'Caddy synchronized: mode=%s upstream=%s backup=%s\n' \
        "$MODE" "$CURRENT_UPSTREAM" "$BACKUP_DIR"
    exit 0
fi

STAGE=build_candidate
build_candidate "$HOST_BEFORE" "$HOST_AFTER"
build_candidate "$ACTIVE_BEFORE" "$ACTIVE_AFTER"

STAGE=write_host
write_regular_file "$HOST_AFTER" "$CADDYFILE"
SWITCHED=true

STAGE=write_active_mount
write_active_file "$ACTIVE_AFTER"

STAGE=verify_upstreams
test "$(grep -Fc "$NEW_UPSTREAM" "$CADDYFILE")" = "$EXPECTED_COUNT"
test "$(docker exec "$CADDY_CONTAINER" grep -Fc "$NEW_UPSTREAM" /etc/caddy/Caddyfile)" = "$EXPECTED_COUNT"
test "$(grep -Fc "$OLD_UPSTREAM" "$CADDYFILE" || true)" = 0
test "$(docker exec "$CADDY_CONTAINER" sh -c "grep -Fc '$OLD_UPSTREAM' /etc/caddy/Caddyfile || true")" = 0

STAGE=validate
docker exec "$CADDY_CONTAINER" caddy validate --config /etc/caddy/Caddyfile --adapter caddyfile >/dev/null

STAGE=reload
docker exec "$CADDY_CONTAINER" caddy reload --config /etc/caddy/Caddyfile --adapter caddyfile >/dev/null

trap - ERR
printf 'Caddy switched: mode=%s target=%s upstream=%s backup=%s\n' \
    "$MODE" "$TARGET" "$NEW_UPSTREAM" "$BACKUP_DIR"
