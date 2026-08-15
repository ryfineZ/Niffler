#!/usr/bin/env bash
set -Eeuo pipefail

SOURCE_CONTAINER="${1:-niffler-background}"
TARGET_CONTAINER="${2:-niffler-background-next}"
APP_PORT="${3:-8085}"
LOG_DIR="${4:-/opt/niffler-app/logs/background-next}"
PGBOUNCER_HOST="${NIFFLER_PGBOUNCER_HOST:-10.72.0.1}"
PGBOUNCER_PORT="${NIFFLER_PGBOUNCER_PORT:-6432}"
PGBOUNCER_DATABASE="${NIFFLER_PGBOUNCER_DATABASE:-}"
STATE_DIR="${NIFFLER_MIGRATION_STATE_DIR:-/opt/niffler-app/migration}"
TIMESTAMP="$(date -u +%Y%m%dT%H%M%SZ)"
BACKUP_DIR="/root/niffler-background-pgbouncer-${TIMESTAMP}"
ENV_FILE="${STATE_DIR}/${TARGET_CONTAINER}.env"
SOURCE_STOPPED=false
TARGET_CREATED=false

rollback_on_error() {
    local exit_code=$?
    if [ "$exit_code" -eq 0 ]; then
        return
    fi
    if [ "$TARGET_CREATED" = true ] && docker inspect "$TARGET_CONTAINER" >/dev/null 2>&1; then
        docker logs "$TARGET_CONTAINER" >"${BACKUP_DIR}/${TARGET_CONTAINER}.log" 2>&1 || true
        chmod 0600 "${BACKUP_DIR}/${TARGET_CONTAINER}.log" 2>/dev/null || true
        docker rm -f "$TARGET_CONTAINER" >/dev/null 2>&1 || true
    fi
    if [ "$SOURCE_STOPPED" = true ]; then
        docker start "$SOURCE_CONTAINER" >/dev/null 2>&1 || true
    fi
    printf 'Background switch failed; old container restart attempted, evidence retained in %s\n' "$BACKUP_DIR" >&2
    exit "$exit_code"
}
trap rollback_on_error ERR

if docker inspect "$TARGET_CONTAINER" >/dev/null 2>&1; then
    printf 'target container already exists: %s\n' "$TARGET_CONTAINER" >&2
    exit 1
fi
test "$(docker inspect --format '{{.State.Status}}' "$SOURCE_CONTAINER")" = running
test "$(docker inspect --format '{{.State.Health.Status}}' "$SOURCE_CONTAINER")" = healthy

IMAGE_ID="$(docker inspect --format '{{.Image}}' "$SOURCE_CONTAINER")"
NETWORK="$(docker inspect "$SOURCE_CONTAINER" | jq -r '.[0].NetworkSettings.Networks | keys | if length == 1 then .[0] else error("source container must have exactly one network") end')"
install -d -m 0700 "$BACKUP_DIR" "$STATE_DIR"
install -d -m 0750 "$LOG_DIR"
docker inspect "$SOURCE_CONTAINER" >"${BACKUP_DIR}/${SOURCE_CONTAINER}.inspect.json"
chmod 0600 "${BACKUP_DIR}/${SOURCE_CONTAINER}.inspect.json"

SOURCE_ENV="${BACKUP_DIR}/${SOURCE_CONTAINER}.env"
docker inspect "$SOURCE_CONTAINER" | jq -r '.[0].Config.Env[]' >"$SOURCE_ENV"
chmod 0600 "$SOURCE_ENV"

python3 - "$SOURCE_ENV" "$ENV_FILE" "$PGBOUNCER_HOST" "$PGBOUNCER_PORT" "$PGBOUNCER_DATABASE" "$APP_PORT" <<'PY'
import os
import sys
from urllib.parse import urlsplit, urlunsplit

source_path, target_path, new_host, new_port, new_database, app_port = sys.argv[1:]
database_keys = {
    "DATABASE_URL",
    "AETHER_DATABASE_URL",
    "AETHER_GATEWAY_DATA_POSTGRES_URL",
}
fixed_values = {
    "AETHER_GATEWAY_DATA_POSTGRES_ACQUIRE_TIMEOUT_MS": "30000",
    "AETHER_GATEWAY_AUTO_PREPARE_DATABASE": "false",
    "AETHER_GATEWAY_DATA_POSTGRES_REQUIRE_SSL": "true",
    "APP_PORT": app_port,
    "BACKGROUND_APP_PORT": app_port,
    "AETHER_GATEWAY_NODE_ROLE": "background",
}
seen = set()
output = []

with open(source_path, encoding="utf-8") as source:
    for raw_line in source:
        line = raw_line.rstrip("\n")
        if "=" not in line:
            raise SystemExit("invalid source environment entry")
        key, value = line.split("=", 1)
        if key in database_keys:
            parsed = urlsplit(value)
            if parsed.scheme not in {"postgres", "postgresql"} or not parsed.hostname:
                raise SystemExit(f"invalid PostgreSQL URL in {key}")
            at = parsed.netloc.rfind("@")
            credentials = parsed.netloc[: at + 1] if at >= 0 else ""
            database_path = f"/{new_database}" if new_database else parsed.path
            value = urlunsplit(
                (parsed.scheme, f"{credentials}{new_host}:{new_port}", database_path, parsed.query, parsed.fragment)
            )
            seen.add(key)
        elif key in fixed_values:
            value = fixed_values[key]
            seen.add(key)
        output.append(f"{key}={value}\n")

required = database_keys | set(fixed_values)
missing = sorted(required - seen)
if missing:
    raise SystemExit(f"required environment variables missing: {','.join(missing)}")

temporary_path = f"{target_path}.tmp.{os.getpid()}"
descriptor = os.open(temporary_path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
with os.fdopen(descriptor, "w", encoding="utf-8") as target:
    target.writelines(output)
    target.flush()
    os.fsync(target.fileno())
os.replace(temporary_path, target_path)
os.chmod(target_path, 0o600)
PY

docker stop --time 30 "$SOURCE_CONTAINER" >/dev/null
SOURCE_STOPPED=true
docker run -d \
    --name "$TARGET_CONTAINER" \
    --restart unless-stopped \
    --network "$NETWORK" \
    --env-file "$ENV_FILE" \
    --mount "type=bind,source=${LOG_DIR},target=/app/logs" \
    --label niffler.migration=pgbouncer \
    --label "niffler.migration.source=${SOURCE_CONTAINER}" \
    "$IMAGE_ID" >/dev/null
TARGET_CREATED=true

for _ in $(seq 1 60); do
    STATUS="$(docker inspect --format '{{.State.Status}}' "$TARGET_CONTAINER")"
    HEALTH="$(docker inspect --format '{{if .State.Health}}{{.State.Health.Status}}{{else}}none{{end}}' "$TARGET_CONTAINER")"
    if [ "$STATUS" = running ] && [ "$HEALTH" = healthy ]; then
        break
    fi
    if [ "$STATUS" != running ] || [ "$HEALTH" = unhealthy ]; then
        printf 'new Background became unhealthy: status=%s health=%s\n' "$STATUS" "$HEALTH" >&2
        false
    fi
    sleep 2
done

test "$(docker inspect --format '{{.State.Status}}' "$TARGET_CONTAINER")" = running
test "$(docker inspect --format '{{.State.Health.Status}}' "$TARGET_CONTAINER")" = healthy
docker exec "$TARGET_CONTAINER" aether-gateway --healthcheck --app-port "$APP_PORT" >/dev/null
test "$(docker inspect --format '{{.State.Status}}' "$SOURCE_CONTAINER")" = exited

SANITIZED_STATE="$(docker inspect "$TARGET_CONTAINER" | jq -r --arg host "$PGBOUNCER_HOST" --arg port "$PGBOUNCER_PORT" --arg database "$PGBOUNCER_DATABASE" '
    [.[0].Config.Env[]
      | select(test("^(DATABASE_URL|AETHER_DATABASE_URL|AETHER_GATEWAY_DATA_POSTGRES_URL)="))
      | capture("^[^=]+=(?<scheme>postgres(?:ql)?)://.*@(?<host>[^:/]+):(?<port>[0-9]+)/(?<database>[^?]+)")
      | select(.host == $host and .port == $port and ($database == "" or .database == $database))] | length')"
test "$SANITIZED_STATE" = 3

trap - ERR
printf 'Background switched: old=%s(exited) new=%s(healthy) image=%s database=%s:%s/%s\n' \
    "$SOURCE_CONTAINER" "$TARGET_CONTAINER" "$IMAGE_ID" "$PGBOUNCER_HOST" "$PGBOUNCER_PORT" "${PGBOUNCER_DATABASE:-source}"
