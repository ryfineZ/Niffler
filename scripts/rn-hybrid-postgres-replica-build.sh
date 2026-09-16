#!/usr/bin/env bash
set -Eeuo pipefail

RN01_HOST="rn01"
HYBRID_HOST="rn-hybrid"
POSTGRES_IMAGE="postgres@sha256:67dc02dae6e27fa8b4333df9bfdf15265b33ce04186cd7e65c0b8fe67ee37b97"
REPLICATION_ROLE="niffler_replica"
REPLICATION_SLOT="niffler_rn_hybrid"
DATA_DIR="/opt/niffler-data/postgres15"
TLS_DIR="/opt/niffler-data/postgres15-tls"
SECRETS_DIR="/opt/niffler-data/secrets"
COMPOSE_FILE="/opt/niffler-data/docker-compose.yml"
RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"
TEMP_DIR="$(mktemp -d)"

cleanup() {
  rm -rf "${TEMP_DIR}"
}
trap cleanup EXIT

ssh_run() {
  local host="$1"
  shift
  ssh -o BatchMode=yes -o ConnectTimeout=10 "${host}" "$@"
}

ssh_run "${RN01_HOST}" 'set -eu; test "$(docker inspect -f "{{.State.Health.Status}}" niffler-postgres)" = healthy; test "$(docker exec niffler-postgres psql -U postgres -Atqc "show server_version_num")" = "150018"; test "$(docker exec niffler-postgres psql -U postgres -Atqc "select pg_is_in_recovery()")" = f'
ssh_run "${HYBRID_HOST}" 'set -eu; test "$(systemctl is-active docker)" = active; test "$(systemctl is-active wg-quick@wg-db)" = active; pg_isready -h 10.72.0.2 -p 5432 -t 5 >/dev/null; test ! -e /opt/niffler-data/postgres15/PG_VERSION'

ssh_run "${RN01_HOST}" 'bash -s' <<'REMOTE_PRIMARY'
set -Eeuo pipefail

HBA_FILE="/opt/niffler-data/postgres-tls/pg_hba.conf"
SECRET_FILE="/root/.niffler-replication-password"
BACKUP_DIR="/root/niffler-replica-prepare-$(date -u +%Y%m%dT%H%M%SZ)"
HBA_LINE="hostssl replication     niffler_replica 192.129.155.207/32      scram-sha-256"

install -d -m 0700 "${BACKUP_DIR}"
cp -a "${HBA_FILE}" "${BACKUP_DIR}/pg_hba.conf"
docker exec niffler-postgres psql -U postgres -Atqc 'show max_slot_wal_keep_size' > "${BACKUP_DIR}/max_slot_wal_keep_size.before"

if ! test -s "${SECRET_FILE}"; then
  umask 077
  openssl rand -hex 32 > "${SECRET_FILE}"
fi
chmod 0600 "${SECRET_FILE}"
replication_password="$(tr -d '\r\n' < "${SECRET_FILE}")"

if ! docker exec niffler-postgres psql -U postgres -Atqc "select 1 from pg_roles where rolname='niffler_replica'" | grep -qx 1; then
  docker exec niffler-postgres psql -v ON_ERROR_STOP=1 -U postgres -c 'CREATE ROLE niffler_replica WITH LOGIN REPLICATION;'
fi
printf "ALTER ROLE niffler_replica WITH LOGIN REPLICATION PASSWORD '%s';\n" "${replication_password}" \
  | docker exec -i niffler-postgres psql -v ON_ERROR_STOP=1 -U postgres >/dev/null
unset replication_password

temporary_hba="$(mktemp "${HBA_FILE}.XXXXXX")"
awk -v new_rule="${HBA_LINE}" '
  BEGIN { inserted=0 }
  $1 == "hostssl" && $2 == "replication" && $3 == "niffler_replica" { next }
  /^hostssl[[:space:]]+all/ && !inserted { print new_rule; inserted=1 }
  { print }
  END { if (!inserted) print new_rule }
' "${HBA_FILE}" > "${temporary_hba}"
chown --reference="${HBA_FILE}" "${temporary_hba}"
chmod --reference="${HBA_FILE}" "${temporary_hba}"
cat "${temporary_hba}" > "${HBA_FILE}"
rm -f "${temporary_hba}"

# Docker keeps the old inode when a bind-mounted single file is atomically replaced.
# Synchronize that already-mounted inode without restarting the production database.
if ! docker exec niffler-postgres grep -Fqx "${HBA_LINE}" /etc/postgresql/tls/pg_hba.conf; then
  postgres_pid="$(docker inspect -f '{{.State.Pid}}' niffler-postgres)"
  nsenter -t "${postgres_pid}" -m -r -- \
    mount -o remount,rw,bind /etc/postgresql/tls/pg_hba.conf /etc/postgresql/tls/pg_hba.conf
  if ! cat "${HBA_FILE}" | nsenter -t "${postgres_pid}" -m -r -- \
    sh -c 'cat > /etc/postgresql/tls/pg_hba.conf'; then
    nsenter -t "${postgres_pid}" -m -r -- \
      mount -o remount,ro,bind /etc/postgresql/tls/pg_hba.conf /etc/postgresql/tls/pg_hba.conf || true
    exit 1
  fi
  nsenter -t "${postgres_pid}" -m -r -- \
    mount -o remount,ro,bind /etc/postgresql/tls/pg_hba.conf /etc/postgresql/tls/pg_hba.conf
fi

docker exec niffler-postgres psql -v ON_ERROR_STOP=1 -U postgres -c "ALTER SYSTEM SET max_slot_wal_keep_size = '8GB';" >/dev/null
docker exec niffler-postgres psql -v ON_ERROR_STOP=1 -U postgres -Atqc 'select pg_reload_conf()' | grep -qx t
test "$(docker exec niffler-postgres psql -U postgres -Atqc 'show max_slot_wal_keep_size')" = "8GB"
test "$(docker exec niffler-postgres psql -U postgres -Atqc "select rolreplication::text||'/'||rolcanlogin::text from pg_roles where rolname='niffler_replica'")" = "true/true"
grep -Fqx "${HBA_LINE}" "${HBA_FILE}"
docker exec niffler-postgres grep -Fqx "${HBA_LINE}" /etc/postgresql/tls/pg_hba.conf

printf 'primary_prepare_backup=%s\n' "${BACKUP_DIR}"
REMOTE_PRIMARY

replication_password="$(ssh_run "${RN01_HOST}" 'tr -d "\r\n" < /root/.niffler-replication-password')"
printf '10.72.0.2:5432:*:%s:%s\n' "${REPLICATION_ROLE}" "${replication_password}" \
  | ssh_run "${HYBRID_HOST}" "set -eu; install -d -m 0700 '${SECRETS_DIR}'; temporary=\$(mktemp '${SECRETS_DIR}/replication.pgpass.XXXXXX'); cat > \"\${temporary}\"; chmod 0600 \"\${temporary}\"; chown 999:999 \"\${temporary}\"; mv \"\${temporary}\" '${SECRETS_DIR}/replication.pgpass'"
unset replication_password

cat > "${TEMP_DIR}/docker-compose.yml" <<EOF
services:
  niffler-postgres15:
    image: ${POSTGRES_IMAGE}
    container_name: niffler-postgres15
    restart: unless-stopped
    stop_grace_period: 2m
    shm_size: 1gb
    command:
      - postgres
      - -c
      - max_connections=100
      - -c
      - shared_buffers=3GB
      - -c
      - effective_cache_size=10GB
      - -c
      - work_mem=8MB
      - -c
      - maintenance_work_mem=512MB
      - -c
      - max_wal_size=4GB
      - -c
      - min_wal_size=1GB
      - -c
      - checkpoint_completion_target=0.9
      - -c
      - hot_standby_feedback=on
      - -c
      - idle_in_transaction_session_timeout=30000
      - -c
      - tcp_keepalives_idle=30
      - -c
      - tcp_keepalives_interval=10
      - -c
      - ssl=on
      - -c
      - ssl_cert_file=/etc/postgresql/tls/server.crt
      - -c
      - ssl_key_file=/etc/postgresql/tls/server.key
      - -c
      - hba_file=/etc/postgresql/tls/pg_hba.conf
    ports:
      - 127.0.0.1:55432:5432
    volumes:
      - ${DATA_DIR}:/var/lib/postgresql/data
      - ${TLS_DIR}/server.crt:/etc/postgresql/tls/server.crt:ro
      - ${TLS_DIR}/server.key:/etc/postgresql/tls/server.key:ro
      - ${TLS_DIR}/pg_hba.conf:/etc/postgresql/tls/pg_hba.conf:ro
      - ${SECRETS_DIR}/replication.pgpass:/run/secrets/replication.pgpass:ro
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U postgres -d aether"]
      interval: 10s
      timeout: 5s
      retries: 12
      start_period: 30s
    ulimits:
      nofile:
        soft: 65536
        hard: 65536
    logging:
      driver: json-file
      options:
        max-size: 20m
        max-file: "5"
EOF

cat > "${TEMP_DIR}/pg_hba.conf" <<'EOF'
local   all             all                                     trust
host    all             all             127.0.0.1/32            trust
host    all             all             ::1/128                 trust
local   replication     all                                     trust
host    replication     all             127.0.0.1/32            trust
host    replication     all             ::1/128                 trust
hostssl all             all             0.0.0.0/0               scram-sha-256
hostssl all             all             ::0/0                   scram-sha-256
hostnossl all           all             0.0.0.0/0               reject
hostnossl all           all             ::0/0                   reject
EOF

scp -q "${TEMP_DIR}/docker-compose.yml" "${TEMP_DIR}/pg_hba.conf" "${HYBRID_HOST}:/tmp/"
ssh_run "${HYBRID_HOST}" "set -eu; docker pull '${POSTGRES_IMAGE}' >/dev/null; test \"\$(docker run --rm '${POSTGRES_IMAGE}' id -u postgres)\" = 999; install -d -m 0700 -o 999 -g 999 '${DATA_DIR}'; test -z \"\$(find '${DATA_DIR}' -mindepth 1 -maxdepth 1 -print -quit)\"; install -d -m 0750 -o root -g 999 '${TLS_DIR}'; install -m 0640 -o root -g 999 /tmp/pg_hba.conf '${TLS_DIR}/pg_hba.conf'; if ! test -s '${TLS_DIR}/server.key' || ! test -s '${TLS_DIR}/server.crt'; then openssl req -x509 -newkey rsa:3072 -sha256 -nodes -days 825 -subj '/CN=rn-hybrid-niffler-postgres15' -addext 'subjectAltName=IP:127.0.0.1,IP:10.72.0.1,DNS:rn-hybrid' -keyout '${TLS_DIR}/server.key' -out '${TLS_DIR}/server.crt' >/dev/null 2>&1; fi; chown 999:999 '${TLS_DIR}/server.key'; chmod 0600 '${TLS_DIR}/server.key'; chown root:999 '${TLS_DIR}/server.crt'; chmod 0640 '${TLS_DIR}/server.crt'; install -m 0600 /tmp/docker-compose.yml '${COMPOSE_FILE}'; rm -f /tmp/docker-compose.yml /tmp/pg_hba.conf; docker compose -f '${COMPOSE_FILE}' config --quiet"

slot_count="$(ssh_run "${RN01_HOST}" "docker exec niffler-postgres psql -U postgres -Atqc \"select count(*) from pg_replication_slots where slot_name='${REPLICATION_SLOT}'\"")"
test "${slot_count}" = 0

ssh_run "${HYBRID_HOST}" "docker run --rm --name niffler-pg-basebackup --network host --user 999:999 -e PGPASSFILE=/run/secrets/replication.pgpass -v '${DATA_DIR}:/var/lib/postgresql/data' -v '${SECRETS_DIR}/replication.pgpass:/run/secrets/replication.pgpass:ro' '${POSTGRES_IMAGE}' pg_basebackup --dbname='host=10.72.0.2 port=5432 user=${REPLICATION_ROLE} sslmode=require passfile=/run/secrets/replication.pgpass application_name=rn-hybrid-standby' --pgdata=/var/lib/postgresql/data --format=plain --wal-method=stream --write-recovery-conf --create-slot --slot='${REPLICATION_SLOT}' --checkpoint=spread --max-rate=49152 --progress --verbose --no-password"

ssh_run "${HYBRID_HOST}" "set -eu; docker compose -f '${COMPOSE_FILE}' up -d; for attempt in \$(seq 1 36); do if test \"\$(docker inspect -f '{{.State.Health.Status}}' niffler-postgres15 2>/dev/null || true)\" = healthy; then break; fi; sleep 5; done; test \"\$(docker inspect -f '{{.State.Health.Status}}' niffler-postgres15)\" = healthy; test \"\$(docker exec niffler-postgres15 psql -U postgres -Atqc 'show server_version_num')\" = 150018; test \"\$(docker exec niffler-postgres15 psql -U postgres -Atqc 'select pg_is_in_recovery()')\" = t; test \"\$(docker exec niffler-postgres15 psql -U postgres -Atqc 'select status from pg_stat_wal_receiver')\" = streaming"

primary_system_id="$(ssh_run "${RN01_HOST}" "docker exec niffler-postgres pg_controldata /var/lib/postgresql/data | awk -F: '/Database system identifier/ { gsub(/ /, \"\", \$2); print \$2 }'")"
standby_system_id="$(ssh_run "${HYBRID_HOST}" "docker exec niffler-postgres15 pg_controldata /var/lib/postgresql/data | awk -F: '/Database system identifier/ { gsub(/ /, \"\", \$2); print \$2 }'")"
test -n "${primary_system_id}"
test "${primary_system_id}" = "${standby_system_id}"

ssh_run "${RN01_HOST}" "docker exec niffler-postgres psql -U postgres -Atqc \"select application_name||'/'||state||'/'||sync_state from pg_stat_replication where application_name='rn-hybrid-standby'; select slot_name||'/'||active from pg_replication_slots where slot_name='${REPLICATION_SLOT}';\""
ssh_run "${HYBRID_HOST}" "docker exec niffler-postgres15 psql -U postgres -Atqc \"select pg_is_in_recovery(),status,coalesce(pg_wal_lsn_diff(latest_end_lsn,pg_last_wal_replay_lsn()),0)::bigint from pg_stat_wal_receiver;\""

printf 'standby ready: PostgreSQL 15.18 system_identifier=%s slot=%s\n' "${standby_system_id}" "${REPLICATION_SLOT}"
