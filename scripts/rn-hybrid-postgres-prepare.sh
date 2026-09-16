#!/bin/bash

set -euo pipefail

if [ "$(id -u)" -ne 0 ]; then
    echo "ERROR: run this script as root" >&2
    exit 1
fi

RN01_PUBLIC_IP="${RN01_PUBLIC_IP:-192.129.155.207}"
HD0526_PUBLIC_IP="${HD0526_PUBLIC_IP:-23.19.228.223}"
OVH_PUBLIC_IP="${OVH_PUBLIC_IP:-15.204.120.221}"
WG_PORT="${WG_PORT:-51821}"

for value in "$RN01_PUBLIC_IP" "$HD0526_PUBLIC_IP" "$OVH_PUBLIC_IP"; do
    if ! [[ "$value" =~ ^[0-9]+(\.[0-9]+){3}$ ]]; then
        echo "ERROR: invalid IPv4 address: $value" >&2
        exit 1
    fi
done
if ! [[ "$WG_PORT" =~ ^[0-9]+$ ]] || [ "$WG_PORT" -lt 1 ] || [ "$WG_PORT" -gt 65535 ]; then
    echo "ERROR: invalid WireGuard port: $WG_PORT" >&2
    exit 1
fi

export DEBIAN_FRONTEND=noninteractive

timestamp="$(date -u '+%Y%m%dT%H%M%SZ')"
backup_dir="/root/niffler-rn-hybrid-prepare-$timestamp"
install -d -m 0700 "$backup_dir"

backup_path() {
    local path="$1"
    if [ -e "$path" ] || [ -L "$path" ]; then
        cp -a --parents "$path" "$backup_dir"
    fi
}

backup_path /etc/apt
backup_path /etc/docker
backup_path /etc/fail2ban
backup_path /etc/sysctl.d
backup_path /etc/ufw
backup_path /etc/postgresql
backup_path /etc/nginx
backup_path /etc/systemd/system/session2sub2api.service

apt-get update
apt-get -y full-upgrade
apt-get install -y \
    ca-certificates \
    curl \
    dnsutils \
    docker-compose-v2 \
    docker.io \
    fail2ban \
    jq \
    lsof \
    pgbouncer \
    rclone \
    rsync \
    unattended-upgrades \
    ufw \
    wireguard-tools

install -d -m 0755 /etc/docker
cat > /etc/docker/daemon.json <<'EOF'
{
  "live-restore": true,
  "log-driver": "json-file",
  "log-opts": {
    "max-file": "5",
    "max-size": "50m"
  }
}
EOF

install -d -m 0755 /etc/fail2ban/jail.d
cat > /etc/fail2ban/jail.d/sshd.local <<'EOF'
[sshd]
enabled = true
backend = systemd
bantime = 1h
findtime = 10m
maxretry = 5
EOF

cat > /etc/sysctl.d/90-niffler-database.conf <<'EOF'
kernel.dmesg_restrict = 1
kernel.kptr_restrict = 2
net.ipv4.conf.all.accept_redirects = 0
net.ipv4.conf.default.accept_redirects = 0
net.ipv4.conf.all.send_redirects = 0
net.ipv4.conf.default.send_redirects = 0
net.ipv6.conf.all.accept_redirects = 0
net.ipv6.conf.default.accept_redirects = 0
vm.swappiness = 1
EOF

cat > /etc/apt/apt.conf.d/20auto-upgrades <<'EOF'
APT::Periodic::Update-Package-Lists "1";
APT::Periodic::Unattended-Upgrade "1";
EOF

sysctl --system >/dev/null
systemctl enable docker
systemctl restart docker
systemctl enable --now fail2ban unattended-upgrades

# The distribution package starts PgBouncer with a placeholder configuration.
# Keep it stopped until the reviewed Niffler configuration is installed.
systemctl disable --now pgbouncer 2>/dev/null || true

ufw default deny incoming
ufw default allow outgoing
ufw allow 22/tcp
ufw allow 80/tcp
ufw allow 443/tcp
ufw allow from "$RN01_PUBLIC_IP" to any port "$WG_PORT" proto udp
ufw allow from "$HD0526_PUBLIC_IP" to any port "$WG_PORT" proto udp
ufw allow from "$OVH_PUBLIC_IP" to any port "$WG_PORT" proto udp
ufw logging low
ufw --force enable

install -d -o root -g root -m 0700 /opt/niffler-data
install -d -o root -g root -m 0750 /opt/niffler-data/logs

docker run --rm hello-world >/dev/null

printf 'backup_dir=%s\n' "$backup_dir"
printf 'docker_version=%s\n' "$(docker version --format '{{.Server.Version}}')"
printf 'compose_version=%s\n' "$(docker compose version --short)"
printf 'pgbouncer_version=%s\n' "$(pgbouncer --version | head -n 1)"
printf 'ufw_status=%s\n' "$(ufw status | head -n 1)"
printf 'fail2ban_status=%s\n' "$(systemctl is-active fail2ban)"
printf 'reboot_required=%s\n' "$([ -f /var/run/reboot-required ] && echo yes || echo no)"
