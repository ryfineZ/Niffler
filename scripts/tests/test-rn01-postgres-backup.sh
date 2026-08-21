#!/bin/bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
TEST_ROOT="$(mktemp -d)"
FAKE_BIN="$TEST_ROOT/bin"
DUMP_PID_FILE="$TEST_ROOT/dump.pid"
DUMP_TERM_FILE="$TEST_ROOT/dump-terminated"
CONTAINER_TERM_FILE="$TEST_ROOT/container-pg-dump-terminated"
TEST_SCRIPT="$TEST_ROOT/rn01-postgres-backup.sh"

cleanup() {
    if [ -f "$DUMP_PID_FILE" ]; then
        dump_pid="$(cat "$DUMP_PID_FILE")"
        kill -TERM "$dump_pid" >/dev/null 2>&1 || true
    fi
    rm -rf -- "$TEST_ROOT"
}
trap cleanup EXIT

mkdir -p "$FAKE_BIN" "$TEST_ROOT/backups" "$TEST_ROOT/status"
sed 's/if \[ "$EUID" -ne 0 \]; then/if false; then/' \
    "$PROJECT_ROOT/scripts/rn01-postgres-backup.sh" > "$TEST_SCRIPT"
chmod +x "$TEST_SCRIPT"
cat > "$TEST_ROOT/r2.env" <<'EOF'
R2_BUCKET=test-bucket
R2_ENDPOINT=https://example.invalid
AWS_ACCESS_KEY_ID=test-key
AWS_SECRET_ACCESS_KEY=test-secret
EOF
chmod 0600 "$TEST_ROOT/r2.env"

cat > "$FAKE_BIN/docker" <<'EOF'
#!/bin/bash
set -euo pipefail

case "${1:-}" in
    inspect)
        exit 0
        ;;
    exec)
        shift
        if [ "${1:-}" = "-i" ]; then
            shift
        fi
        shift
        case " ${*:-} " in
            *" pg_isready "*)
                exit 0
                ;;
            *" psql "*)
                printf '1024\n'
                exit 0
                ;;
            *" kill -TERM "*)
                : > "$CONTAINER_TERM_FILE"
                if [ -f "$DUMP_PID_FILE" ]; then
                    kill -TERM "$(cat "$DUMP_PID_FILE")" >/dev/null 2>&1 || true
                fi
                exit 0
                ;;
            *" pg_dump "*)
                printf '%s\n' "$$" > "$DUMP_PID_FILE"
                trap ': > "$DUMP_TERM_FILE"; exit 143' TERM INT
                while true; do
                    sleep 1
                done
                ;;
        esac
        ;;
esac

exit 1
EOF

cat > "$FAKE_BIN/rclone" <<'EOF'
#!/bin/bash
exit 0
EOF

cat > "$FAKE_BIN/df" <<'EOF'
#!/bin/bash
printf 'Avail\n999999999999\n'
EOF

cat > "$FAKE_BIN/flock" <<'EOF'
#!/bin/bash
exit 0
EOF

cat > "$FAKE_BIN/stat" <<'EOF'
#!/bin/bash
case "${1:-} ${2:-}" in
    '-c %a') printf '600\n' ;;
    '-c %s') printf '1\n' ;;
    *) exit 1 ;;
esac
EOF

cat > "$FAKE_BIN/sha256sum" <<'EOF'
#!/bin/bash
printf '%064d  %s\n' 0 "${1:-backup.dump}"
EOF

chmod +x "$FAKE_BIN/docker" "$FAKE_BIN/rclone" "$FAKE_BIN/df" \
    "$FAKE_BIN/flock" "$FAKE_BIN/stat" "$FAKE_BIN/sha256sum"

export DUMP_PID_FILE DUMP_TERM_FILE CONTAINER_TERM_FILE
export PATH="$FAKE_BIN:$PATH"
export NIFFLER_BACKUP_CONFIG_FILE="$TEST_ROOT/r2.env"
export NIFFLER_BACKUP_DIR="$TEST_ROOT/backups"
export NIFFLER_BACKUP_STATUS_DIR="$TEST_ROOT/status"
export NIFFLER_BACKUP_LOCK_FILE="$TEST_ROOT/backup.lock"
export NIFFLER_POSTGRES_CONTAINER=test-postgres

"$TEST_SCRIPT" > "$TEST_ROOT/output.log" 2>&1 &
backup_pid=$!

for _ in $(seq 1 50); do
    if [ -f "$DUMP_PID_FILE" ] && grep -Fxq 'STATUS=running' "$TEST_ROOT/status/status.env"; then
        break
    fi
    sleep 0.1
done

test -f "$DUMP_PID_FILE"
grep -Fxq 'STATUS=running' "$TEST_ROOT/status/status.env"

kill -TERM "$backup_pid"
set +e
wait "$backup_pid"
exit_code=$?
set -e

test "$exit_code" -ne 0
grep -Fxq 'STATUS=failed' "$TEST_ROOT/status/status.env"
test -f "$DUMP_TERM_FILE"
test -f "$CONTAINER_TERM_FILE"
if kill -0 "$(cat "$DUMP_PID_FILE")" >/dev/null 2>&1; then
    echo "pg_dump process remained alive after backup termination" >&2
    exit 1
fi
if find "$TEST_ROOT/backups" -type f -name '*.partial' -print -quit | grep -q .; then
    echo "partial backup remained after termination" >&2
    exit 1
fi

echo "backup signal handling test passed"
