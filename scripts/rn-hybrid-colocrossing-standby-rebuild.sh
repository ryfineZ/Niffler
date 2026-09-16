#!/usr/bin/env bash
set -Eeuo pipefail

PRIMARY_HOST="colocrossing-la-db"
STANDBY_HOST="rn-hybrid"
POSTGRES_IMAGE="postgres@sha256:67dc02dae6e27fa8b4333df9bfdf15265b33ce04186cd7e65c0b8fe67ee37b97"
REPLICATION_ROLE="niffler_colo_repl"
REPLICATION_SLOT="niffler_rn_hybrid"
DATA_DIR="/opt/niffler-data/postgres15"
TLS_DIR="/opt/niffler-data/postgres15-tls"
SECRETS_DIR="/opt/niffler-data/secrets"
COMPOSE_FILE="/opt/niffler-data/docker-compose.yml"
PRIMARY_PGPASS="/opt/niffler-data/secrets/source-replication.pgpass"
RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"
TEMP_DIR="$(mktemp -d)"

cleanup() {
  rm -rf "${TEMP_DIR}"
}
trap cleanup EXIT
umask 077

ssh_run() {
  local host="$1"
  shift
  ssh -o BatchMode=yes -o ConnectTimeout=10 "${host}" "$@"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cp "${PROJECT_ROOT}/deploy/rn-hybrid/postgres-standby-compose.yml" "${TEMP_DIR}/docker-compose.yml"
cp "${PROJECT_ROOT}/deploy/rn-hybrid/pg_hba.standby.conf" "${TEMP_DIR}/pg_hba.conf"

primary_state="$(ssh_run "${PRIMARY_HOST}" "sudo docker exec niffler-postgres15 psql -p 55432 -U postgres -AtF '|' -c \"SELECT pg_is_in_recovery(), current_setting('transaction_read_only'), (SELECT count(*) FROM pg_replication_slots WHERE slot_name='${REPLICATION_SLOT}');\"")"
test "${primary_state}" = "f|off|0"
primary_system_id="$(ssh_run "${PRIMARY_HOST}" "sudo docker exec niffler-postgres15 pg_controldata /var/lib/postgresql/data/pgdata | awk -F: '/Database system identifier/ {gsub(/ /,\"\",\$2); print \$2}'")"
primary_timeline="$(ssh_run "${PRIMARY_HOST}" "sudo docker exec niffler-postgres15 pg_controldata /var/lib/postgresql/data/pgdata | awk -F: \"/Latest checkpoint.s TimeLineID/ {gsub(/ /,\\\"\\\",\\\$2); print \\\$2}\"")"
test -n "${primary_system_id}"
test "${primary_timeline}" = "3"

ssh_run "${STANDBY_HOST}" "set -eu; test \"\$(systemctl is-active docker)\" = active; test \"\$(systemctl is-active wg-quick@wg-db)\" = active; test \"\$(docker inspect -f '{{.State.Status}}/{{.HostConfig.RestartPolicy.Name}}' niffler-postgres15)\" = exited/no; test \"\$(systemctl is-active pgbouncer)\" = inactive; test \"\$(systemctl is-active niffler-postgres-replication-relay.service)\" = inactive; nc -z -w 5 10.72.0.5 55432; test -s '${DATA_DIR}/PG_VERSION'"

old_control="$(ssh_run "${STANDBY_HOST}" "sudo docker run --rm -v '${DATA_DIR}:/data:ro' '${POSTGRES_IMAGE}' pg_controldata /data")"
grep -Fq "Database cluster state:               shut down" <<<"${old_control}"
grep -Fq "Latest checkpoint's TimeLineID:       2" <<<"${old_control}"
grep -Fq "Database system identifier:           ${primary_system_id}" <<<"${old_control}"

ssh_run "${PRIMARY_HOST}" "sudo cat '${PRIMARY_PGPASS}'" > "${TEMP_DIR}/source-replication.pgpass"
awk -F: -v OFS=: -v host="10.72.0.5" -v port="55432" -v role="${REPLICATION_ROLE}" '
  NF >= 5 && $4 == role {$1=host; $2=port; print; found=1}
  END {if (!found) exit 1}
' "${TEMP_DIR}/source-replication.pgpass" > "${TEMP_DIR}/replication.pgpass"
chmod 0600 "${TEMP_DIR}/replication.pgpass"

scp -q "${TEMP_DIR}/docker-compose.yml" "${TEMP_DIR}/pg_hba.conf" "${TEMP_DIR}/replication.pgpass" "${STANDBY_HOST}:/tmp/"

old_data_dir="${DATA_DIR}.pre-colo-primary-${RUN_ID}"
ssh_run "${STANDBY_HOST}" "sudo bash -s" <<REMOTE_PREPARE
set -Eeuo pipefail
backup_dir="/root/niffler-rn-standby-rebuild-${RUN_ID}"
install -d -m 0700 "\${backup_dir}"
cp -a "${COMPOSE_FILE}" "${TLS_DIR}/pg_hba.conf" "${SECRETS_DIR}/replication.pgpass" "\${backup_dir}/"
docker update --restart=no niffler-postgres15 >/dev/null
test "\$(docker inspect -f '{{.State.Status}}' niffler-postgres15)" = exited
test ! -e "${old_data_dir}"
mv "${DATA_DIR}" "${old_data_dir}"
chown root:root "${old_data_dir}"
chmod 0500 "${old_data_dir}"
install -d -m 0700 -o 999 -g 999 "${DATA_DIR}"
install -m 0600 -o root -g root /tmp/docker-compose.yml "${COMPOSE_FILE}"
install -m 0640 -o root -g 999 /tmp/pg_hba.conf "${TLS_DIR}/pg_hba.conf"
install -m 0600 -o 999 -g 999 /tmp/replication.pgpass "${SECRETS_DIR}/replication.pgpass"
rm -f /tmp/docker-compose.yml /tmp/pg_hba.conf /tmp/replication.pgpass
docker compose -f "${COMPOSE_FILE}" config --quiet
printf 'old_data_dir=%s\nbackup_dir=%s\n' "${old_data_dir}" "\${backup_dir}"
REMOTE_PREPARE

ssh_run "${STANDBY_HOST}" "sudo docker run --rm --name niffler-pg-basebackup --network host --user 999:999 -e PGPASSFILE=/run/secrets/replication.pgpass -v '${DATA_DIR}:/var/lib/postgresql/data' -v '${SECRETS_DIR}/replication.pgpass:/run/secrets/replication.pgpass:ro' '${POSTGRES_IMAGE}' pg_basebackup --dbname='host=10.72.0.5 port=55432 user=${REPLICATION_ROLE} sslmode=require passfile=/run/secrets/replication.pgpass application_name=rn-hybrid-standby' --pgdata=/var/lib/postgresql/data --format=plain --wal-method=stream --write-recovery-conf --create-slot --slot='${REPLICATION_SLOT}' --checkpoint=spread --max-rate=49152 --progress --verbose --no-password"

ssh_run "${STANDBY_HOST}" "set -eu; sudo docker compose -f '${COMPOSE_FILE}' up -d --force-recreate; for attempt in \$(seq 1 48); do if test \"\$(docker inspect -f '{{if .State.Health}}{{.State.Health.Status}}{{end}}' niffler-postgres15 2>/dev/null || true)\" = healthy; then break; fi; sleep 5; done; test \"\$(docker inspect -f '{{.State.Health.Status}}' niffler-postgres15)\" = healthy; test \"\$(docker exec niffler-postgres15 psql -U postgres -Atqc 'show server_version_num')\" = 150018; test \"\$(docker exec niffler-postgres15 psql -U postgres -Atqc 'select pg_is_in_recovery()')\" = t; test \"\$(docker exec niffler-postgres15 psql -U postgres -Atqc 'select status from pg_stat_wal_receiver')\" = streaming"

standby_system_id="$(ssh_run "${STANDBY_HOST}" "docker exec niffler-postgres15 pg_controldata /var/lib/postgresql/data | awk -F: '/Database system identifier/ {gsub(/ /,\"\",\$2); print \$2}'")"
test "${standby_system_id}" = "${primary_system_id}"

ssh_run "${PRIMARY_HOST}" "sudo docker exec niffler-postgres15 psql -p 55432 -U postgres -AtF '|' -c \"SELECT application_name,state,sync_state FROM pg_stat_replication WHERE application_name='rn-hybrid-standby'; SELECT slot_name,active,wal_status FROM pg_replication_slots WHERE slot_name='${REPLICATION_SLOT}';\""
ssh_run "${STANDBY_HOST}" "docker exec niffler-postgres15 psql -U postgres -AtF '|' -c \"SELECT pg_is_in_recovery(),status,coalesce(pg_wal_lsn_diff(latest_end_lsn,pg_last_wal_replay_lsn()),0)::bigint FROM pg_stat_wal_receiver;\""

printf 'standby ready: system_identifier=%s slot=%s old_data=%s\n' "${standby_system_id}" "${REPLICATION_SLOT}" "${old_data_dir}"
