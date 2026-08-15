#!/usr/bin/env bash
set -Eeuo pipefail

HYBRID_HOST="rn-hybrid"
RN01_HOST="rn01"
HD_HOST="hd0526"
OVH_HOST="ovh-US-WEST-OR-VPS-4"
HYBRID_PUBLIC_IP="192.255.151.28"
WG_PORT="51821"
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

preflight_host() {
  local host="$1"
  ssh_run "${host}" \
    'set -eu; command -v wg >/dev/null; command -v wg-quick >/dev/null; command -v systemctl >/dev/null; test "$(systemctl is-active ssh 2>/dev/null || systemctl is-active sshd 2>/dev/null)" = active'
}

backup_host() {
  local host="$1"
  local backup_dir="/root/niffler-wg-db-${RUN_ID}"
  ssh_run "${host}" "set -eu; install -d -m 0700 '${backup_dir}'; if test -d /etc/wireguard; then cp -a /etc/wireguard '${backup_dir}/'; fi; for unit in /etc/systemd/system/niffler-postgres-migration-proxy.socket /etc/systemd/system/niffler-postgres-migration-proxy.service; do if test -e \"\${unit}\"; then cp -a \"\${unit}\" '${backup_dir}/'; fi; done; printf '%s\\n' '${backup_dir}'"
}

ensure_key_and_get_public() {
  local host="$1"
  local public_key
  public_key="$(ssh_run "${host}" 'set -eu; umask 077; install -d -m 0700 /etc/wireguard; if ! test -s /etc/wireguard/niffler-db-private.key; then wg genkey > /etc/wireguard/niffler-db-private.key; fi; chmod 0600 /etc/wireguard/niffler-db-private.key; wg pubkey < /etc/wireguard/niffler-db-private.key' | tail -n 1)"
  if [[ ! "${public_key}" =~ ^[A-Za-z0-9+/]{43}=$ ]]; then
    printf 'invalid WireGuard public key returned by %s\n' "${host}" >&2
    return 1
  fi
  printf '%s' "${public_key}"
}

install_wg_config() {
  local host="$1"
  local source_file="$2"
  local remote_template="/tmp/niffler-wg-db-${RUN_ID}.conf"
  scp -q "${source_file}" "${host}:${remote_template}"
  ssh_run "${host}" "set -eu; target=/etc/wireguard/wg-db.conf; temporary=\$(mktemp /etc/wireguard/wg-db.conf.XXXXXX); private_key=\$(tr -d '\\r\\n' < /etc/wireguard/niffler-db-private.key); sed \"s|__LOCAL_PRIVATE_KEY__|\${private_key}|\" '${remote_template}' > \"\${temporary}\"; unset private_key; chmod 0600 \"\${temporary}\"; mv \"\${temporary}\" \"\${target}\"; rm -f '${remote_template}'"
}

enable_wg() {
  local host="$1"
  ssh_run "${host}" 'set -eu; systemctl enable wg-quick@wg-db.service >/dev/null; if systemctl is-active --quiet wg-quick@wg-db.service; then systemctl restart wg-quick@wg-db.service; else systemctl start wg-quick@wg-db.service; fi; test "$(systemctl is-active wg-quick@wg-db.service)" = active'
}

for host in "${HYBRID_HOST}" "${RN01_HOST}" "${HD_HOST}" "${OVH_HOST}"; do
  preflight_host "${host}"
done

for host in "${HYBRID_HOST}" "${RN01_HOST}" "${HD_HOST}" "${OVH_HOST}"; do
  printf '%s backup=%s\n' "${host}" "$(backup_host "${host}")"
done

HYBRID_PUBLIC_KEY="$(ensure_key_and_get_public "${HYBRID_HOST}")"
RN01_PUBLIC_KEY="$(ensure_key_and_get_public "${RN01_HOST}")"
HD_PUBLIC_KEY="$(ensure_key_and_get_public "${HD_HOST}")"
OVH_PUBLIC_KEY="$(ensure_key_and_get_public "${OVH_HOST}")"

cat > "${TEMP_DIR}/hybrid.conf" <<EOF
[Interface]
Address = 10.72.0.1/24
ListenPort = ${WG_PORT}
PrivateKey = __LOCAL_PRIVATE_KEY__
SaveConfig = false

[Peer]
PublicKey = ${RN01_PUBLIC_KEY}
AllowedIPs = 10.72.0.2/32

[Peer]
PublicKey = ${HD_PUBLIC_KEY}
AllowedIPs = 10.72.0.3/32

[Peer]
PublicKey = ${OVH_PUBLIC_KEY}
AllowedIPs = 10.72.0.4/32
EOF

cat > "${TEMP_DIR}/rn01.conf" <<EOF
[Interface]
Address = 10.72.0.2/32
PrivateKey = __LOCAL_PRIVATE_KEY__
SaveConfig = false
PostUp = iptables -I INPUT 1 -i %i -s 10.72.0.1/32 -d 10.72.0.2/32 -p tcp --dport 5432 -j ACCEPT
PostUp = iptables -I INPUT 2 -i %i -d 10.72.0.2/32 -p tcp --dport 5432 -j DROP
PostDown = iptables -D INPUT -i %i -d 10.72.0.2/32 -p tcp --dport 5432 -j DROP || true
PostDown = iptables -D INPUT -i %i -s 10.72.0.1/32 -d 10.72.0.2/32 -p tcp --dport 5432 -j ACCEPT || true

[Peer]
PublicKey = ${HYBRID_PUBLIC_KEY}
Endpoint = ${HYBRID_PUBLIC_IP}:${WG_PORT}
AllowedIPs = 10.72.0.1/32
PersistentKeepalive = 25
EOF

cat > "${TEMP_DIR}/hd.conf" <<EOF
[Interface]
Address = 10.72.0.3/32
PrivateKey = __LOCAL_PRIVATE_KEY__
SaveConfig = false

[Peer]
PublicKey = ${HYBRID_PUBLIC_KEY}
Endpoint = ${HYBRID_PUBLIC_IP}:${WG_PORT}
AllowedIPs = 10.72.0.1/32
PersistentKeepalive = 25
EOF

cat > "${TEMP_DIR}/ovh.conf" <<EOF
[Interface]
Address = 10.72.0.4/32
PrivateKey = __LOCAL_PRIVATE_KEY__
SaveConfig = false

[Peer]
PublicKey = ${HYBRID_PUBLIC_KEY}
Endpoint = ${HYBRID_PUBLIC_IP}:${WG_PORT}
AllowedIPs = 10.72.0.1/32
PersistentKeepalive = 25
EOF

install_wg_config "${HYBRID_HOST}" "${TEMP_DIR}/hybrid.conf"
install_wg_config "${RN01_HOST}" "${TEMP_DIR}/rn01.conf"
install_wg_config "${HD_HOST}" "${TEMP_DIR}/hd.conf"
install_wg_config "${OVH_HOST}" "${TEMP_DIR}/ovh.conf"

enable_wg "${HYBRID_HOST}"
ssh_run "${HYBRID_HOST}" 'ufw allow in on wg-db from 10.72.0.3 to 10.72.0.1 port 6432 proto tcp >/dev/null; ufw allow in on wg-db from 10.72.0.4 to 10.72.0.1 port 6432 proto tcp >/dev/null'
enable_wg "${RN01_HOST}"
enable_wg "${HD_HOST}"
enable_wg "${OVH_HOST}"

cat > "${TEMP_DIR}/niffler-postgres-migration-proxy.socket" <<'EOF'
[Unit]
Description=Niffler migration PostgreSQL private socket
BindsTo=wg-quick@wg-db.service
After=wg-quick@wg-db.service

[Socket]
ListenStream=10.72.0.2:5432
FreeBind=yes
NoDelay=yes

[Install]
WantedBy=sockets.target
EOF

cat > "${TEMP_DIR}/niffler-postgres-migration-proxy.service" <<'EOF'
[Unit]
Description=Niffler migration PostgreSQL TCP proxy
Requires=wg-quick@wg-db.service
After=wg-quick@wg-db.service

[Service]
ExecStart=/usr/lib/systemd/systemd-socket-proxyd 192.129.155.207:5432
NoNewPrivileges=yes
PrivateTmp=yes
ProtectHome=yes
ProtectSystem=strict
EOF

scp -q "${TEMP_DIR}/niffler-postgres-migration-proxy.socket" "${TEMP_DIR}/niffler-postgres-migration-proxy.service" "${RN01_HOST}:/tmp/"
ssh_run "${RN01_HOST}" 'set -eu; install -m 0644 /tmp/niffler-postgres-migration-proxy.socket /etc/systemd/system/niffler-postgres-migration-proxy.socket; install -m 0644 /tmp/niffler-postgres-migration-proxy.service /etc/systemd/system/niffler-postgres-migration-proxy.service; rm -f /tmp/niffler-postgres-migration-proxy.socket /tmp/niffler-postgres-migration-proxy.service; systemd-analyze verify /etc/systemd/system/niffler-postgres-migration-proxy.socket /etc/systemd/system/niffler-postgres-migration-proxy.service; systemctl daemon-reload; systemctl enable --now niffler-postgres-migration-proxy.socket; test "$(systemctl is-active niffler-postgres-migration-proxy.socket)" = active; timeout 5 bash -lc "</dev/tcp/192.129.155.207/5432"'

ssh_run "${RN01_HOST}" 'ping -c 3 -W 2 10.72.0.1 >/dev/null'
ssh_run "${HD_HOST}" 'ping -c 3 -W 2 10.72.0.1 >/dev/null'
ssh_run "${OVH_HOST}" 'ping -c 3 -W 2 10.72.0.1 >/dev/null'
ssh_run "${HYBRID_HOST}" 'ping -c 3 -W 2 10.72.0.2 >/dev/null; ping -c 3 -W 2 10.72.0.3 >/dev/null; ping -c 3 -W 2 10.72.0.4 >/dev/null; pg_isready -h 10.72.0.2 -p 5432 -t 5'

ssh_run "${HYBRID_HOST}" 'wg show wg-db latest-handshakes | awk '\''BEGIN { count=0; bad=0 } { count++; if ($2 == 0) bad=1 } END { if (count != 3 || bad) exit 1 }'\'''
for host in "${RN01_HOST}" "${HD_HOST}" "${OVH_HOST}"; do
  ssh_run "${host}" 'wg show wg-db latest-handshakes | awk '\''BEGIN { count=0; bad=0 } { count++; if ($2 == 0) bad=1 } END { if (count != 1 || bad) exit 1 }'\'''
done

ssh_run "${RN01_HOST}" 'ss -ltn | grep -F "10.72.0.2:5432" >/dev/null'
ssh_run "${HYBRID_HOST}" 'ufw status | grep -F "10.72.0.3" >/dev/null; ufw status | grep -F "10.72.0.4" >/dev/null'

printf 'wg-db ready: rn-hybrid=10.72.0.1 rn01=10.72.0.2 hd0526=10.72.0.3 ovh=10.72.0.4\n'
