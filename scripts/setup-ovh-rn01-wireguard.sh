#!/bin/bash

set -euo pipefail

RN_HOST="${RN_HOST:-rn01}"
OVH_HOST="${OVH_HOST:-ovh-US-WEST-OR-VPS-4}"
RN_PUBLIC_IP="${RN_PUBLIC_IP:-192.129.155.207}"
OVH_PUBLIC_IP="${OVH_PUBLIC_IP:-15.204.120.221}"
RN_WG_IP="10.71.0.1"
OVH_WG_IP="10.71.0.2"
WG_PORT="51820"

ssh -o BatchMode=yes "$RN_HOST" \
    'apt-get update >/dev/null && DEBIAN_FRONTEND=noninteractive apt-get install -y wireguard-tools >/dev/null'

for host in "$RN_HOST" "$OVH_HOST"; do
    ssh -o BatchMode=yes "$host" 'bash -s' <<'REMOTE'
set -euo pipefail
install -d -m 0700 /etc/wireguard
if [ ! -s /etc/wireguard/niffler-private.key ]; then
    umask 077
    wg genkey > /etc/wireguard/niffler-private.key
fi
chmod 0600 /etc/wireguard/niffler-private.key
REMOTE
done

rn_public_key="$(
    ssh -o BatchMode=yes "$RN_HOST" \
        'wg pubkey < /etc/wireguard/niffler-private.key'
)"
ovh_public_key="$(
    ssh -o BatchMode=yes "$OVH_HOST" \
        'wg pubkey < /etc/wireguard/niffler-private.key'
)"

ssh -o BatchMode=yes "$RN_HOST" \
    "OVH_PUBLIC_KEY='$ovh_public_key' OVH_PUBLIC_IP='$OVH_PUBLIC_IP' RN_PUBLIC_IP='$RN_PUBLIC_IP' bash -s" <<'REMOTE'
set -euo pipefail

timestamp="$(date -u '+%Y%m%dT%H%M%SZ')"
backup_dir="/root/niffler-wireguard-backup-$timestamp"
install -d -m 0700 "$backup_dir"
[ ! -e /etc/wireguard/wg0.conf ] || cp -a /etc/wireguard/wg0.conf "$backup_dir/"
cp -a /etc/iptables/rules.v4 "$backup_dir/rules.v4"

private_key="$(cat /etc/wireguard/niffler-private.key)"
cat > /etc/wireguard/wg0.conf <<EOF
[Interface]
Address = 10.71.0.1/30
ListenPort = 51820
PrivateKey = $private_key

[Peer]
PublicKey = $OVH_PUBLIC_KEY
AllowedIPs = 10.71.0.2/32
EOF
chmod 0600 /etc/wireguard/wg0.conf

iptables -C INPUT -s "$OVH_PUBLIC_IP/32" -p udp --dport 51820 -j ACCEPT 2>/dev/null || \
    iptables -I INPUT 1 -s "$OVH_PUBLIC_IP/32" -p udp --dport 51820 -j ACCEPT
iptables -C INPUT -p udp --dport 51820 -j DROP 2>/dev/null || \
    iptables -I INPUT 2 -p udp --dport 51820 -j DROP
iptables -C INPUT -s 10.71.0.2/32 -i wg0 -p tcp -m multiport --dports 5432,6379 -j ACCEPT 2>/dev/null || \
    iptables -I INPUT 1 -s 10.71.0.2/32 -i wg0 -p tcp -m multiport --dports 5432,6379 -j ACCEPT

# Remove rules created by the previous script version, which incorrectly used
# rn01's own public address as a trusted remote source.
while iptables -C INPUT -s "$RN_PUBLIC_IP/32" -p tcp -m multiport --dports 5432,6379 -j ACCEPT 2>/dev/null; do
    iptables -D INPUT -s "$RN_PUBLIC_IP/32" -p tcp -m multiport --dports 5432,6379 -j ACCEPT
done
for port in 5432 6379; do
    while iptables -C DOCKER-USER -s "$RN_PUBLIC_IP/32" -p tcp -m conntrack --ctorigdstport "$port" --ctdir ORIGINAL -j ACCEPT 2>/dev/null; do
        iptables -D DOCKER-USER -s "$RN_PUBLIC_IP/32" -p tcp -m conntrack --ctorigdstport "$port" --ctdir ORIGINAL -j ACCEPT
    done
done
netfilter-persistent save >/dev/null

cat > /etc/systemd/system/niffler-postgres-proxy.socket <<'EOF'
[Unit]
Description=Niffler PostgreSQL private WireGuard listener
After=wg-quick@wg0.service
Requires=wg-quick@wg0.service

[Socket]
ListenStream=10.71.0.1:5432
FreeBind=true
NoDelay=true

[Install]
WantedBy=sockets.target
EOF

cat > /etc/systemd/system/niffler-postgres-proxy.service <<'EOF'
[Unit]
Description=Niffler PostgreSQL private WireGuard proxy

[Service]
ExecStart=/lib/systemd/systemd-socket-proxyd 192.129.155.207:5432
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
NoNewPrivileges=true
EOF

cat > /etc/systemd/system/niffler-redis-proxy.socket <<'EOF'
[Unit]
Description=Niffler Redis private WireGuard listener
After=wg-quick@wg0.service
Requires=wg-quick@wg0.service

[Socket]
ListenStream=10.71.0.1:6379
FreeBind=true
NoDelay=true

[Install]
WantedBy=sockets.target
EOF

cat > /etc/systemd/system/niffler-redis-proxy.service <<'EOF'
[Unit]
Description=Niffler Redis private WireGuard proxy

[Service]
ExecStart=/lib/systemd/systemd-socket-proxyd 192.129.155.207:6379
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
NoNewPrivileges=true
EOF

systemctl daemon-reload
systemctl enable wg-quick@wg0
systemctl restart wg-quick@wg0
systemctl enable --now niffler-postgres-proxy.socket niffler-redis-proxy.socket
printf 'backup_dir=%s\n' "$backup_dir"
REMOTE

ssh -o BatchMode=yes "$OVH_HOST" \
    "RN_PUBLIC_KEY='$rn_public_key' RN_PUBLIC_IP='$RN_PUBLIC_IP' bash -s" <<'REMOTE'
set -euo pipefail

timestamp="$(date -u '+%Y%m%dT%H%M%SZ')"
backup_dir="/root/niffler-wireguard-backup-$timestamp"
install -d -m 0700 "$backup_dir"
[ ! -e /etc/wireguard/wg0.conf ] || cp -a /etc/wireguard/wg0.conf "$backup_dir/"

private_key="$(cat /etc/wireguard/niffler-private.key)"
cat > /etc/wireguard/wg0.conf <<EOF
[Interface]
Address = 10.71.0.2/30
PrivateKey = $private_key

[Peer]
PublicKey = $RN_PUBLIC_KEY
Endpoint = $RN_PUBLIC_IP:51820
AllowedIPs = 10.71.0.1/32
PersistentKeepalive = 25
EOF
chmod 0600 /etc/wireguard/wg0.conf

systemctl enable wg-quick@wg0
systemctl restart wg-quick@wg0
printf 'backup_dir=%s\n' "$backup_dir"
REMOTE

ssh -o BatchMode=yes "$OVH_HOST" 'bash -s' <<'REMOTE'
set -euo pipefail
ping -c 3 -W 2 10.71.0.1 >/dev/null
timeout 5 bash -c '</dev/tcp/10.71.0.1/5432'
timeout 5 bash -c '</dev/tcp/10.71.0.1/6379'
printf 'wireguard=ready\n'
REMOTE
