#!/bin/bash

set -Eeuo pipefail

CONFIG_FILE="${NIFFLER_BACKUP_CONFIG_FILE:-/etc/niffler-backup/r2.env}"
STATUS_FILE="${NIFFLER_BACKUP_STATUS_FILE:-/var/lib/niffler-backup/status.env}"
VERIFY_STATUS_FILE="${NIFFLER_RESTORE_VERIFY_STATUS_FILE:-/var/lib/niffler-backup/restore-verified.env}"
VERIFY_LOG_FILE="${NIFFLER_RESTORE_VERIFY_LOG_FILE:-/var/lib/niffler-backup/restore-verify.log}"
POSTGRES_IMAGE="postgres@sha256:67dc02dae6e27fa8b4333df9bfdf15265b33ce04186cd7e65c0b8fe67ee37b97"
RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"
DOWNLOAD_DIR="/var/backups/niffler-restore-validation/${RUN_ID}"
RESTORE_DATA="/opt/niffler-data/restore-validation-${RUN_ID}"
RESTORE_CONTAINER="niffler-postgres-restore-${RUN_ID,,}"
backup_id=""
object_key=""
expected_sha256=""
stage="initializing"
success=0

die() {
    printf 'ERROR: %s\n' "$*" | tee -a "$VERIFY_LOG_FILE" >&2
    exit 1
}

read_status_value() {
    local key="$1"
    awk -F= -v key="$key" '$1 == key { sub(/^[^=]*=/, ""); print; exit }' "$STATUS_FILE"
}

write_failure_status() {
    local temporary_status="${VERIFY_STATUS_FILE}.tmp.$$"
    {
        printf 'STATUS=failed\n'
        printf 'FAILED_AT=%s\n' "$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
        printf 'STAGE=%s\n' "$stage"
        printf 'BACKUP_ID=%s\n' "$backup_id"
        printf 'OBJECT_KEY=%s\n' "$object_key"
        printf 'DOWNLOAD_DIR=%s\n' "$DOWNLOAD_DIR"
        printf 'RESTORE_DATA=%s\n' "$RESTORE_DATA"
        printf 'RESTORE_CONTAINER=%s\n' "$RESTORE_CONTAINER"
        printf 'LOG_FILE=%s\n' "$VERIFY_LOG_FILE"
    } > "$temporary_status"
    chmod 0600 "$temporary_status"
    mv "$temporary_status" "$VERIFY_STATUS_FILE"
}

finish() {
    local exit_code=$?
    trap - EXIT
    if [ "$success" -eq 1 ] && [ "$exit_code" -eq 0 ]; then
        docker rm -f "$RESTORE_CONTAINER" >/dev/null 2>&1 || true
        case "$DOWNLOAD_DIR" in
            /var/backups/niffler-restore-validation/*) rm -rf -- "$DOWNLOAD_DIR" ;;
        esac
        case "$RESTORE_DATA" in
            /opt/niffler-data/restore-validation-*) rm -rf -- "$RESTORE_DATA" ;;
        esac
    else
        docker stop --time 30 "$RESTORE_CONTAINER" >/dev/null 2>&1 || true
        write_failure_status
        printf 'restore verification failed at stage=%s; artifacts retained\n' "$stage" | tee -a "$VERIFY_LOG_FILE" >&2
    fi
    exit "$exit_code"
}
trap finish EXIT

if [ "$EUID" -ne 0 ]; then
    die "must run as root"
fi
for command_name in docker rclone sha256sum stat; do
    command -v "$command_name" >/dev/null 2>&1 || die "required command is unavailable: $command_name"
done
install -d -m 0700 "$(dirname "$VERIFY_STATUS_FILE")" "$(dirname "$VERIFY_LOG_FILE")"
: > "$VERIFY_LOG_FILE"
chmod 0600 "$VERIFY_LOG_FILE"
printf 'started_at=%s run_id=%s\n' "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" "$RUN_ID" >> "$VERIFY_LOG_FILE"
if [ ! -f "$CONFIG_FILE" ] || [ -L "$CONFIG_FILE" ] || [ "$(stat -c '%a' "$CONFIG_FILE")" != 600 ]; then
    die "backup credential file is missing or unsafe"
fi
if [ ! -f "$STATUS_FILE" ] || [ -L "$STATUS_FILE" ] || [ "$(stat -c '%a' "$STATUS_FILE")" != 600 ]; then
    die "backup status file is missing or unsafe"
fi

backup_status="$(read_status_value STATUS)"
backup_id="$(read_status_value BACKUP_ID)"
object_key="$(read_status_value OBJECT_KEY)"
expected_sha256="$(read_status_value BACKUP_SHA256)"
test "$backup_status" = success || die "latest backup did not finish successfully"
[[ "$backup_id" =~ ^[0-9]{8}T[0-9]{6}Z$ ]] || die "backup id is invalid"
[[ "$object_key" =~ ^postgres/aether/daily/[0-9]{4}/[0-9]{2}/aether-[0-9]{8}T[0-9]{6}Z\.dump$ ]] \
    || die "backup object key is invalid"
[[ "$expected_sha256" =~ ^[a-f0-9]{64}$ ]] || die "backup checksum is invalid"

set -a
# shellcheck disable=SC1090
source "$CONFIG_FILE"
set +a
: "${R2_BUCKET:?}"
: "${R2_ENDPOINT:?}"
: "${AWS_ACCESS_KEY_ID:?}"
: "${AWS_SECRET_ACCESS_KEY:?}"

export RCLONE_CONFIG_R2_TYPE=s3
export RCLONE_CONFIG_R2_PROVIDER=Cloudflare
export RCLONE_CONFIG_R2_ACCESS_KEY_ID="$AWS_ACCESS_KEY_ID"
export RCLONE_CONFIG_R2_SECRET_ACCESS_KEY="$AWS_SECRET_ACCESS_KEY"
export RCLONE_CONFIG_R2_ENDPOINT="$R2_ENDPOINT"
export RCLONE_CONFIG_R2_REGION=auto
export RCLONE_CONFIG_R2_NO_CHECK_BUCKET=true

available_bytes="$(df --output=avail -B1 /opt/niffler-data | tail -n 1 | tr -d ' ')"
if [ "$available_bytes" -lt 107374182400 ]; then
    die "less than 100 GiB is available for isolated restore"
fi

install -d -m 0700 "$DOWNLOAD_DIR"
install -d -m 0700 -o 999 -g 999 "$RESTORE_DATA"
dump_name="$(basename "$object_key")"
dump_path="$DOWNLOAD_DIR/$dump_name"
checksum_path="$dump_path.sha256"

stage="download"
printf 'stage=%s\n' "$stage" >> "$VERIFY_LOG_FILE"
rclone copyto "r2:$R2_BUCKET/$object_key" "$dump_path" --retries 3 --low-level-retries 10 2>> "$VERIFY_LOG_FILE"
rclone copyto "r2:$R2_BUCKET/$object_key.sha256" "$checksum_path" --retries 3 --low-level-retries 10 2>> "$VERIFY_LOG_FILE"
chmod 0600 "$dump_path" "$checksum_path"
stage="checksum"
printf 'stage=%s\n' "$stage" >> "$VERIFY_LOG_FILE"
actual_sha256="$(sha256sum "$dump_path" | awk '{print $1}')"
test "$actual_sha256" = "$expected_sha256" || die "downloaded backup checksum does not match status"
(
    cd "$DOWNLOAD_DIR"
    sha256sum --check "$(basename "$checksum_path")" >/dev/null
)

stage="initialize"
printf 'stage=%s\n' "$stage" >> "$VERIFY_LOG_FILE"
docker run -d \
    --name "$RESTORE_CONTAINER" \
    --shm-size 1g \
    --mount "type=bind,src=$RESTORE_DATA,dst=/var/lib/postgresql/data" \
    -e POSTGRES_HOST_AUTH_METHOD=trust \
    "$POSTGRES_IMAGE" >/dev/null

for attempt in $(seq 1 60); do
    if docker exec "$RESTORE_CONTAINER" pg_isready -U postgres -d postgres >/dev/null 2>&1; then
        break
    fi
    sleep 2
done
docker exec "$RESTORE_CONTAINER" pg_isready -U postgres -d postgres >/dev/null \
    || die "isolated PostgreSQL did not become ready"
docker exec "$RESTORE_CONTAINER" createdb -U postgres restore_aether
stage="pg_restore"
printf 'stage=%s\n' "$stage" >> "$VERIFY_LOG_FILE"
if ! docker exec -i "$RESTORE_CONTAINER" pg_restore \
    --verbose \
    --exit-on-error \
    --no-owner \
    --no-privileges \
    -U postgres \
    -d restore_aether < "$dump_path" >> "$VERIFY_LOG_FILE" 2>&1; then
    docker logs "$RESTORE_CONTAINER" >> "$VERIFY_LOG_FILE" 2>&1 || true
    tail -n 80 "$VERIFY_LOG_FILE" >&2
    die "pg_restore failed"
fi

stage="validate"
printf 'stage=%s\n' "$stage" >> "$VERIFY_LOG_FILE"
validation="$(
    docker exec "$RESTORE_CONTAINER" psql -X -U postgres -d restore_aether -AtF '|' -c \
        "SELECT
            (SELECT count(*) FROM pg_tables WHERE schemaname = 'public'),
            (SELECT count(*) FROM _sqlx_migrations),
            (SELECT count(*) FROM users),
            (SELECT count(*) FROM api_keys),
            (SELECT count(*) FROM providers),
            EXISTS (SELECT 1 FROM usage LIMIT 1);"
)"
IFS='|' read -r table_count migration_count user_count api_key_count provider_count usage_exists <<< "$validation"
printf 'validation=%s\n' "$validation" >> "$VERIFY_LOG_FILE"
test "$table_count" -ge 100 || die "restored public table count is too small"
test "$migration_count" -ge 60 || die "restored migration count is too small"
test "$user_count" -gt 0 || die "restored users table is empty"
test "$api_key_count" -gt 0 || die "restored api_keys table is empty"
test "$provider_count" -gt 0 || die "restored providers table is empty"
test "$usage_exists" = t || die "restored usage table is empty"

install -d -m 0700 "$(dirname "$VERIFY_STATUS_FILE")"
temporary_status="${VERIFY_STATUS_FILE}.tmp.$$"
{
    printf 'STATUS=success\n'
    printf 'VERIFIED_AT=%s\n' "$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
    printf 'BACKUP_ID=%s\n' "$backup_id"
    printf 'OBJECT_KEY=%s\n' "$object_key"
    printf 'BACKUP_SHA256=%s\n' "$actual_sha256"
    printf 'PUBLIC_TABLES=%s\n' "$table_count"
    printf 'MIGRATIONS=%s\n' "$migration_count"
    printf 'USERS=%s\n' "$user_count"
    printf 'API_KEYS=%s\n' "$api_key_count"
    printf 'PROVIDERS=%s\n' "$provider_count"
    printf 'USAGE_PRESENT=%s\n' "$usage_exists"
} > "$temporary_status"
chmod 0600 "$temporary_status"
mv "$temporary_status" "$VERIFY_STATUS_FILE"
success=1

printf 'restore verified: backup=%s tables=%s migrations=%s users=%s api_keys=%s providers=%s\n' \
    "$backup_id" "$table_count" "$migration_count" "$user_count" "$api_key_count" "$provider_count"
