#!/usr/bin/env bash
set -Eeuo pipefail

RN01_HOST="rn01"
HYBRID_HOST="rn-hybrid"
HD_HOST="hd0526"
OVH_HOST="ovh-US-WEST-OR-VPS-4"
CONFIG_FILE="/etc/pgbouncer/pgbouncer.ini"
USERLIST_FILE="/etc/pgbouncer/userlist.txt"
APP_PGPASS="/root/.niffler-pgbouncer-app.pgpass"
ADMIN_PGPASS="/root/.niffler-pgbouncer-admin.pgpass"
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

ssh_run "${HYBRID_HOST}" 'set -eu; test "$(systemctl is-active wg-quick@wg-db)" = active; test "$(systemctl is-active pgbouncer 2>/dev/null || true)" = inactive; test "$(docker exec niffler-postgres15 psql -U postgres -Atqc "select pg_is_in_recovery()")" = t; test "$(docker exec niffler-postgres15 psql -U postgres -Atqc "select status from pg_stat_wal_receiver")" = streaming'

niffler_verifier="$(ssh_run "${RN01_HOST}" "docker exec niffler-postgres psql -U postgres -Atqc \"select rolpassword from pg_authid where rolname='niffler_app'\"")"
if [[ ! "${niffler_verifier}" =~ ^SCRAM-SHA-256\$[0-9]+: ]]; then
  printf 'niffler_app does not have a usable SCRAM verifier\n' >&2
  exit 1
fi

database_url="$(ssh_run "${HD_HOST}" 'docker inspect -f '\''{{range .Config.Env}}{{println .}}{{end}}'\'' niffler-frontdoor | sed -n '\''s/^DATABASE_URL=//p'\'' | head -n 1')"
parsed_url="$(printf '%s' "${database_url}" | python3 -c 'import sys; from urllib.parse import urlsplit, unquote; u=urlsplit(sys.stdin.read().strip()); print(unquote(u.username or "")); print(unquote(u.password or "")); print((u.path or "").lstrip("/"))')"
unset database_url
app_user="$(printf '%s\n' "${parsed_url}" | sed -n '1p')"
app_password="$(printf '%s\n' "${parsed_url}" | sed -n '2p')"
app_database="$(printf '%s\n' "${parsed_url}" | sed -n '3p')"
unset parsed_url
test "${app_user}" = "niffler_app"
test "${app_database}" = "aether"
test -n "${app_password}"

printf '%s\n' "${niffler_verifier}" \
  | ssh_run "${HYBRID_HOST}" "set -eu; temporary=\$(mktemp /etc/pgbouncer/niffler-app.scram.XXXXXX); cat > \"\${temporary}\"; chmod 0600 \"\${temporary}\"; chown postgres:postgres \"\${temporary}\"; mv \"\${temporary}\" /etc/pgbouncer/niffler-app.scram"
unset niffler_verifier

printf '10.72.0.1:6432:aether:niffler_app:%s\n' "${app_password}" \
  | ssh_run "${HYBRID_HOST}" "set -eu; temporary=\$(mktemp /root/.niffler-pgbouncer-app.pgpass.XXXXXX); cat > \"\${temporary}\"; chmod 0600 \"\${temporary}\"; mv \"\${temporary}\" '${APP_PGPASS}'"
unset app_password app_user app_database

cat > "${TEMP_DIR}/pgbouncer.ini" <<'EOF'
[databases]
aether = host=10.72.0.2 port=5432 dbname=aether
aether_ovh = host=10.72.0.2 port=5432 dbname=aether pool_mode=transaction max_db_connections=10
aether_background = host=10.72.0.2 port=5432 dbname=aether pool_mode=transaction max_db_connections=5

[pgbouncer]
listen_addr = 10.72.0.1
listen_port = 6432
unix_socket_dir = /var/run/postgresql
auth_type = scram-sha-256
auth_file = /etc/pgbouncer/userlist.txt
admin_users = pgbouncer_admin
stats_users = pgbouncer_admin

pool_mode = transaction
max_client_conn = 300
default_pool_size = 40
min_pool_size = 5
reserve_pool_size = 10
reserve_pool_timeout = 3
max_db_connections = 60
max_user_connections = 60
max_prepared_statements = 256
ignore_startup_parameters = extra_float_digits

query_wait_timeout = 120
client_login_timeout = 30
idle_transaction_timeout = 60
server_connect_timeout = 10
server_login_retry = 1
server_idle_timeout = 60
server_lifetime = 3600

client_tls_sslmode = require
client_tls_key_file = /etc/pgbouncer/tls/server.key
client_tls_cert_file = /etc/pgbouncer/tls/server.crt
client_tls_protocols = secure
server_tls_sslmode = require
server_tls_protocols = secure

tcp_keepalive = 1
tcp_keepidle = 30
tcp_keepintvl = 10
tcp_keepcnt = 3
application_name_add_host = 1

logfile = /var/log/postgresql/pgbouncer.log
pidfile = /var/run/postgresql/pgbouncer.pid
log_connections = 0
log_disconnections = 0
log_pooler_errors = 1
stats_period = 60
verbose = 0
EOF

cat > "${TEMP_DIR}/override.conf" <<'EOF'
[Service]
LimitNOFILE=65536
Restart=on-failure
RestartSec=2s
EOF

scp -q "${TEMP_DIR}/pgbouncer.ini" "${TEMP_DIR}/override.conf" "${HYBRID_HOST}:/tmp/"
ssh_run "${HYBRID_HOST}" 'bash -s' <<'REMOTE_PREPARE'
set -Eeuo pipefail

BACKUP_DIR="/root/niffler-pgbouncer-prepare-$(date -u +%Y%m%dT%H%M%SZ)"
ADMIN_PASSWORD_FILE="/root/.niffler-pgbouncer-admin-password"
ADMIN_VERIFIER_FILE="/etc/pgbouncer/pgbouncer-admin.scram"

install -d -m 0700 "${BACKUP_DIR}"
cp -a /etc/pgbouncer "${BACKUP_DIR}/"
if test -d /etc/systemd/system/pgbouncer.service.d; then
  cp -a /etc/systemd/system/pgbouncer.service.d "${BACKUP_DIR}/"
fi

install -d -m 0750 -o root -g postgres /etc/pgbouncer/tls
if ! test -s /etc/pgbouncer/tls/server.key || ! test -s /etc/pgbouncer/tls/server.crt; then
  openssl req -x509 -newkey rsa:3072 -sha256 -nodes -days 825 \
    -subj '/CN=rn-hybrid-niffler-pgbouncer' \
    -addext 'subjectAltName=IP:10.72.0.1,DNS:rn-hybrid' \
    -keyout /etc/pgbouncer/tls/server.key \
    -out /etc/pgbouncer/tls/server.crt >/dev/null 2>&1
fi
chown postgres:postgres /etc/pgbouncer/tls/server.key /etc/pgbouncer/tls/server.crt
chmod 0600 /etc/pgbouncer/tls/server.key
chmod 0640 /etc/pgbouncer/tls/server.crt

if ! test -s "${ADMIN_PASSWORD_FILE}"; then
  umask 077
  openssl rand -hex 32 > "${ADMIN_PASSWORD_FILE}"
fi
chmod 0600 "${ADMIN_PASSWORD_FILE}"

python3 - "${ADMIN_PASSWORD_FILE}" > "${ADMIN_VERIFIER_FILE}.tmp" <<'PY'
import base64
import hashlib
import hmac
import os
import pathlib
import sys

password = pathlib.Path(sys.argv[1]).read_text().strip().encode()
salt = os.urandom(16)
iterations = 4096
salted = hashlib.pbkdf2_hmac("sha256", password, salt, iterations)
client_key = hmac.new(salted, b"Client Key", hashlib.sha256).digest()
stored_key = hashlib.sha256(client_key).digest()
server_key = hmac.new(salted, b"Server Key", hashlib.sha256).digest()
print(
    "SCRAM-SHA-256${}:{}${}:{}".format(
        iterations,
        base64.b64encode(salt).decode(),
        base64.b64encode(stored_key).decode(),
        base64.b64encode(server_key).decode(),
    )
)
PY
chown postgres:postgres "${ADMIN_VERIFIER_FILE}.tmp"
chmod 0600 "${ADMIN_VERIFIER_FILE}.tmp"
mv "${ADMIN_VERIFIER_FILE}.tmp" "${ADMIN_VERIFIER_FILE}"

temporary_userlist="$(mktemp /etc/pgbouncer/userlist.txt.XXXXXX)"
printf '"niffler_app" "%s"\n' "$(cat /etc/pgbouncer/niffler-app.scram)" > "${temporary_userlist}"
printf '"pgbouncer_admin" "%s"\n' "$(cat "${ADMIN_VERIFIER_FILE}")" >> "${temporary_userlist}"
chown postgres:postgres "${temporary_userlist}"
chmod 0600 "${temporary_userlist}"
mv "${temporary_userlist}" /etc/pgbouncer/userlist.txt

temporary_admin_pgpass="$(mktemp /root/.niffler-pgbouncer-admin.pgpass.XXXXXX)"
printf '10.72.0.1:6432:pgbouncer:pgbouncer_admin:%s\n' "$(cat "${ADMIN_PASSWORD_FILE}")" > "${temporary_admin_pgpass}"
chmod 0600 "${temporary_admin_pgpass}"
mv "${temporary_admin_pgpass}" /root/.niffler-pgbouncer-admin.pgpass

install -m 0640 -o root -g postgres /tmp/pgbouncer.ini /etc/pgbouncer/pgbouncer.ini
install -d -m 0755 /etc/systemd/system/pgbouncer.service.d
install -m 0644 /tmp/override.conf /etc/systemd/system/pgbouncer.service.d/override.conf
rm -f /tmp/pgbouncer.ini /tmp/override.conf
systemctl daemon-reload
systemctl enable --now pgbouncer.service
test "$(systemctl is-active pgbouncer.service)" = active
printf 'pgbouncer_backup=%s\n' "${BACKUP_DIR}"
REMOTE_PREPARE

ssh_run "${HYBRID_HOST}" 'bash -s' <<'REMOTE_TEST'
set -Eeuo pipefail

APP_CONN="host=10.72.0.1 port=6432 dbname=aether user=niffler_app sslmode=require connect_timeout=10"
ADMIN_CONN="host=10.72.0.1 port=6432 dbname=pgbouncer user=pgbouncer_admin sslmode=require connect_timeout=10"
APP_PGPASS="/root/.niffler-pgbouncer-app.pgpass"
ADMIN_PGPASS="/root/.niffler-pgbouncer-admin.pgpass"
PAUSE_RESULT="/tmp/niffler-pgbouncer-pause-result.$$"
PGBENCH_SCRIPT="/tmp/niffler-pgbouncer-pgbench.$$"
paused=0

cleanup_test() {
  if [ "${paused}" -eq 1 ]; then
    PGPASSFILE="${ADMIN_PGPASS}" psql "${ADMIN_CONN}" -v ON_ERROR_STOP=1 -qc 'RESUME aether' >/dev/null 2>&1 || true
  fi
  rm -f "${PAUSE_RESULT}" "${PGBENCH_SCRIPT}"
}
trap cleanup_test EXIT

PGPASSFILE="${APP_PGPASS}" psql "${APP_CONN}" -v ON_ERROR_STOP=1 -Atqc \
  "BEGIN; SET LOCAL statement_timeout='5s'; SELECT pg_advisory_xact_lock(7264001); CREATE TEMP TABLE niffler_proxy_probe(value integer) ON COMMIT DROP; INSERT INTO niffler_proxy_probe VALUES (1); SELECT count(*) FROM niffler_proxy_probe; COMMIT;" \
  | grep -qx 1

printf 'SELECT 1;\n' > "${PGBENCH_SCRIPT}"
PGPASSFILE="${APP_PGPASS}" PGSSLMODE=require pgbench \
  -h 10.72.0.1 -p 6432 -U niffler_app -d aether \
  -n -M prepared -c 2 -t 10 -f "${PGBENCH_SCRIPT}" >/dev/null

PGPASSFILE="${ADMIN_PGPASS}" psql "${ADMIN_CONN}" -v ON_ERROR_STOP=1 -qc 'PAUSE aether' >/dev/null
paused=1
PGPASSFILE="${APP_PGPASS}" psql "${APP_CONN}" -v ON_ERROR_STOP=1 -Atqc 'SELECT 4242;' > "${PAUSE_RESULT}" &
waiting_pid=$!
sleep 2
kill -0 "${waiting_pid}"
waiting_clients="$(PGPASSFILE="${ADMIN_PGPASS}" psql "${ADMIN_CONN}" -AtF '|' -c 'SHOW POOLS' | awk -F '|' '$1 == "aether" { total += $4 } END { print total + 0 }')"
test "${waiting_clients}" -ge 1
PGPASSFILE="${ADMIN_PGPASS}" psql "${ADMIN_CONN}" -v ON_ERROR_STOP=1 -qc 'RESUME aether' >/dev/null
paused=0
wait "${waiting_pid}"
grep -qx 4242 "${PAUSE_RESULT}"

test "$(systemctl is-active pgbouncer.service)" = active
ss -ltn | grep -F '10.72.0.1:6432' >/dev/null
test "$(journalctl -u pgbouncer.service --since '-10 minutes' -p err --no-pager -q | wc -l)" -eq 0
printf 'pgbouncer_tests=transaction_write,prepared_statements,pause_queue,resume waiting_clients=%s\n' "${waiting_clients}"
REMOTE_TEST

ssh_run "${HD_HOST}" 'timeout 5 bash -lc "</dev/tcp/10.72.0.1/6432"'
ssh_run "${OVH_HOST}" 'timeout 5 bash -lc "</dev/tcp/10.72.0.1/6432"'
ssh_run "${HYBRID_HOST}" 'systemctl reload pgbouncer.service; test "$(systemctl is-active pgbouncer.service)" = active'

printf 'PgBouncer ready on 10.72.0.1:6432 with rn01 as backend\n'
