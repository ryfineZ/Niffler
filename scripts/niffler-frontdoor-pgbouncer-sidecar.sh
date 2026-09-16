#!/usr/bin/env bash
set -Eeuo pipefail

SOURCE_CONTAINER="${1:?source container is required}"
SIDECAR_CONTAINER="${2:?sidecar container is required}"
HOST_PORT="${3:?host validation port is required}"
CONTAINER_PORT="${4:?container port is required}"
LOG_DIR="${5:?sidecar log directory is required}"
PGBOUNCER_HOST="${NIFFLER_PGBOUNCER_HOST:-10.72.0.1}"
PGBOUNCER_PORT="${NIFFLER_PGBOUNCER_PORT:-6432}"
PGBOUNCER_DATABASE="${NIFFLER_PGBOUNCER_DATABASE:-}"
STATE_DIR="${NIFFLER_MIGRATION_STATE_DIR:-/opt/niffler-app/migration}"
TIMESTAMP="$(date -u +%Y%m%dT%H%M%SZ)"
BACKUP_DIR="/root/niffler-frontdoor-pgbouncer-${TIMESTAMP}"
ENV_FILE="${STATE_DIR}/${SIDECAR_CONTAINER}.env"
CREATED=false

cleanup_on_error() {
    local exit_code=$?
    if [ "$exit_code" -eq 0 ]; then
        return
    fi
    if [ "$CREATED" = true ] && docker inspect "$SIDECAR_CONTAINER" >/dev/null 2>&1; then
        docker logs "$SIDECAR_CONTAINER" >"${BACKUP_DIR}/${SIDECAR_CONTAINER}.log" 2>&1 || true
        chmod 0600 "${BACKUP_DIR}/${SIDECAR_CONTAINER}.log" 2>/dev/null || true
        docker rm -f "$SIDECAR_CONTAINER" >/dev/null 2>&1 || true
    fi
    printf 'sidecar deployment failed; evidence retained in %s\n' "$BACKUP_DIR" >&2
    exit "$exit_code"
}
trap cleanup_on_error ERR

case "$HOST_PORT:$CONTAINER_PORT" in
    *[!0-9:]* | :* | *:) printf 'ports must be numeric\n' >&2; exit 2 ;;
esac

if docker inspect "$SIDECAR_CONTAINER" >/dev/null 2>&1; then
    printf 'container already exists: %s\n' "$SIDECAR_CONTAINER" >&2
    exit 1
fi
if ss -lntH "sport = :${HOST_PORT}" | grep -q .; then
    printf 'host port is already in use: %s\n' "$HOST_PORT" >&2
    exit 1
fi

SOURCE_STATUS="$(docker inspect --format '{{.State.Status}}' "$SOURCE_CONTAINER")"
SOURCE_HEALTH="$(docker inspect --format '{{if .State.Health}}{{.State.Health.Status}}{{else}}none{{end}}' "$SOURCE_CONTAINER")"
if [ "$SOURCE_STATUS" != running ] || [ "$SOURCE_HEALTH" != healthy ]; then
    printf 'source container is not running and healthy: status=%s health=%s\n' "$SOURCE_STATUS" "$SOURCE_HEALTH" >&2
    exit 1
fi

SOURCE_IMAGE_ID="$(docker inspect --format '{{.Image}}' "$SOURCE_CONTAINER")"
IMAGE_REFERENCE="${NIFFLER_SIDECAR_IMAGE:-$SOURCE_IMAGE_ID}"
IMAGE_ID="$(docker image inspect --format '{{.Id}}' "$IMAGE_REFERENCE")"
NETWORK="$(docker inspect "$SOURCE_CONTAINER" | jq -r '.[0].NetworkSettings.Networks | keys | if length == 1 then .[0] else error("source container must have exactly one network") end')"
install -d -m 0700 "$BACKUP_DIR" "$STATE_DIR"
install -d -m 0750 "$LOG_DIR"
docker inspect "$SOURCE_CONTAINER" >"${BACKUP_DIR}/${SOURCE_CONTAINER}.inspect.json"
chmod 0600 "${BACKUP_DIR}/${SOURCE_CONTAINER}.inspect.json"

SOURCE_ENV="${BACKUP_DIR}/${SOURCE_CONTAINER}.env"
docker inspect "$SOURCE_CONTAINER" | jq -r '.[0].Config.Env[]' >"$SOURCE_ENV"
chmod 0600 "$SOURCE_ENV"

python3 - "$SOURCE_ENV" "$ENV_FILE" "$PGBOUNCER_HOST" "$PGBOUNCER_PORT" "$PGBOUNCER_DATABASE" "$CONTAINER_PORT" <<'PY'
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

docker run -d \
    --name "$SIDECAR_CONTAINER" \
    --restart unless-stopped \
    --network "$NETWORK" \
    --env-file "$ENV_FILE" \
    --mount "type=bind,source=${LOG_DIR},target=/app/logs" \
    --publish "127.0.0.1:${HOST_PORT}:${CONTAINER_PORT}" \
    --label niffler.migration=pgbouncer \
    --label "niffler.migration.source=${SOURCE_CONTAINER}" \
    "$IMAGE_ID" >/dev/null
CREATED=true

for _ in $(seq 1 60); do
    STATUS="$(docker inspect --format '{{.State.Status}}' "$SIDECAR_CONTAINER")"
    HEALTH="$(docker inspect --format '{{if .State.Health}}{{.State.Health.Status}}{{else}}none{{end}}' "$SIDECAR_CONTAINER")"
    if [ "$STATUS" = running ] && [ "$HEALTH" = healthy ]; then
        break
    fi
    if [ "$STATUS" != running ] || [ "$HEALTH" = unhealthy ]; then
        printf 'sidecar became unhealthy: status=%s health=%s\n' "$STATUS" "$HEALTH" >&2
        false
    fi
    sleep 2
done

test "$(docker inspect --format '{{.State.Status}}' "$SIDECAR_CONTAINER")" = running
test "$(docker inspect --format '{{.State.Health.Status}}' "$SIDECAR_CONTAINER")" = healthy
curl --fail --silent --show-error --max-time 10 "http://127.0.0.1:${HOST_PORT}/_gateway/health" >/dev/null
curl --fail --silent --show-error --max-time 15 "http://127.0.0.1:${HOST_PORT}/" >/dev/null
curl --fail --silent --show-error --max-time 15 "http://127.0.0.1:${HOST_PORT}/api/public/global-models?limit=1" >/dev/null
curl --fail --silent --show-error --max-time 15 "http://127.0.0.1:${HOST_PORT}/api/oauth/providers" >/dev/null

SANITIZED_STATE="$(docker inspect "$SIDECAR_CONTAINER" | jq -r --arg host "$PGBOUNCER_HOST" --arg port "$PGBOUNCER_PORT" --arg database "$PGBOUNCER_DATABASE" '
    [.[0].Config.Env[]
      | select(test("^(DATABASE_URL|AETHER_DATABASE_URL|AETHER_GATEWAY_DATA_POSTGRES_URL)="))
      | capture("^[^=]+=(?<scheme>postgres(?:ql)?)://.*@(?<host>[^:/]+):(?<port>[0-9]+)/(?<database>[^?]+)")
      | select(.host == $host and .port == $port and ($database == "" or .database == $database))] | length')"
test "$SANITIZED_STATE" = 3

trap - ERR
printf 'sidecar ready: container=%s image=%s network=%s local_port=%s database=%s:%s/%s\n' \
    "$SIDECAR_CONTAINER" "$IMAGE_ID" "$NETWORK" "$HOST_PORT" "$PGBOUNCER_HOST" "$PGBOUNCER_PORT" "${PGBOUNCER_DATABASE:-source}"
