#!/usr/bin/env bash
set -Eeuo pipefail

RN01_HOST="rn01"
HYBRID_HOST="rn-hybrid"
RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

ssh_run() {
  local host="$1"
  shift
  ssh -o BatchMode=yes -o ConnectTimeout=10 "${host}" "$@"
}

ssh_run "${HYBRID_HOST}" 'set -eu; test "$(docker inspect -f "{{.State.Health.Status}}" niffler-postgres15)" = healthy; test "$(docker exec niffler-postgres15 psql -U postgres -Atqc "select pg_is_in_recovery()")" = t; test "$(docker exec niffler-postgres15 psql -U postgres -Atqc "select status from pg_stat_wal_receiver")" = streaming; test "$(systemctl is-active pgbouncer)" = active'
ssh_run "${RN01_HOST}" 'set -eu; test -s /etc/niffler-backup/r2.env; test "$(stat -c "%a" /etc/niffler-backup/r2.env)" = 600; test -s /etc/niffler-backup/telegram.env; test "$(stat -c "%a" /etc/niffler-backup/telegram.env)" = 600'

for credential in r2.env telegram.env; do
  ssh_run "${RN01_HOST}" "cat '/etc/niffler-backup/${credential}'" \
    | ssh_run "${HYBRID_HOST}" "set -eu; install -d -m 0700 /etc/niffler-backup; temporary=\$(mktemp '/etc/niffler-backup/${credential}.XXXXXX'); cat > \"\${temporary}\"; chmod 0600 \"\${temporary}\"; mv \"\${temporary}\" '/etc/niffler-backup/${credential}'"
done

scp -q \
  "${SCRIPT_DIR}/rn01-postgres-backup.sh" \
  "${SCRIPT_DIR}/rn01-postgres-backup-alert.sh" \
  "${SCRIPT_DIR}/rn-hybrid-postgres-backup-restore-verify.sh" \
  "${SCRIPT_DIR}/rn-hybrid-postgres-backup.service" \
  "${SCRIPT_DIR}/rn-hybrid-postgres-backup.timer" \
  "${SCRIPT_DIR}/rn-hybrid-postgres-backup-alert.service" \
  "${HYBRID_HOST}:/tmp/"

ssh_run "${HYBRID_HOST}" "set -eu; backup_dir='/root/niffler-backup-deploy-${RUN_ID}'; install -d -m 0700 \"\${backup_dir}\"; for path in /usr/local/sbin/niffler-postgres-backup /usr/local/sbin/niffler-postgres-backup-alert /usr/local/sbin/niffler-postgres-backup-restore-verify /etc/systemd/system/niffler-postgres-backup.service /etc/systemd/system/niffler-postgres-backup.timer /etc/systemd/system/niffler-postgres-backup-alert.service; do if test -e \"\${path}\"; then cp -a \"\${path}\" \"\${backup_dir}/\"; fi; done; install -m 0755 /tmp/rn01-postgres-backup.sh /usr/local/sbin/niffler-postgres-backup; install -m 0755 /tmp/rn01-postgres-backup-alert.sh /usr/local/sbin/niffler-postgres-backup-alert; install -m 0755 /tmp/rn-hybrid-postgres-backup-restore-verify.sh /usr/local/sbin/niffler-postgres-backup-restore-verify; install -m 0644 /tmp/rn-hybrid-postgres-backup.service /etc/systemd/system/niffler-postgres-backup.service; install -m 0644 /tmp/rn-hybrid-postgres-backup.timer /etc/systemd/system/niffler-postgres-backup.timer; install -m 0644 /tmp/rn-hybrid-postgres-backup-alert.service /etc/systemd/system/niffler-postgres-backup-alert.service; rm -f /tmp/rn01-postgres-backup.sh /tmp/rn01-postgres-backup-alert.sh /tmp/rn-hybrid-postgres-backup-restore-verify.sh /tmp/rn-hybrid-postgres-backup.service /tmp/rn-hybrid-postgres-backup.timer /tmp/rn-hybrid-postgres-backup-alert.service; systemd-analyze verify /etc/systemd/system/niffler-postgres-backup.service /etc/systemd/system/niffler-postgres-backup.timer /etc/systemd/system/niffler-postgres-backup-alert.service; systemctl daemon-reload; systemctl disable --now niffler-postgres-backup.timer >/dev/null 2>&1 || true; printf 'backup_deploy_backup=%s\\n' \"\${backup_dir}\""

ssh_run "${HYBRID_HOST}" 'set -eu; systemctl start --no-block niffler-postgres-backup.service; for attempt in $(seq 1 1440); do state=$(systemctl show niffler-postgres-backup.service -p ActiveState --value); if test "$state" = inactive || test "$state" = failed; then break; fi; sleep 5; done; test "$(systemctl show niffler-postgres-backup.service -p Result --value)" = success'
ssh_run "${HYBRID_HOST}" 'set -eu; test "$(systemctl show niffler-postgres-backup.service -p Result --value)" = success; grep -Fxq STATUS=success /var/lib/niffler-backup/status.env; /usr/local/sbin/niffler-postgres-backup-restore-verify; grep -Fxq STATUS=success /var/lib/niffler-backup/restore-verified.env; systemctl enable --now niffler-postgres-backup.timer; test "$(systemctl is-active niffler-postgres-backup.timer)" = active'

ssh_run "${RN01_HOST}" 'systemctl disable --now niffler-postgres-backup.timer; test "$(systemctl is-enabled niffler-postgres-backup.timer 2>/dev/null || true)" = disabled; test "$(systemctl is-active niffler-postgres-backup.timer 2>/dev/null || true)" = inactive'

ssh_run "${HYBRID_HOST}" 'systemctl list-timers --all niffler-postgres-backup.timer --no-pager; sed -n "1,20p" /var/lib/niffler-backup/status.env; sed -n "1,30p" /var/lib/niffler-backup/restore-verified.env'
printf 'rn-hybrid R2 backup and isolated restore verified; rn01 timer disabled\n'
