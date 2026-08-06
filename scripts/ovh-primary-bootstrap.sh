#!/bin/bash

set -euo pipefail

if [ "$(id -u)" -ne 0 ]; then
    echo "ERROR: run this script as root" >&2
    exit 1
fi

export DEBIAN_FRONTEND=noninteractive

timestamp="$(date -u '+%Y%m%dT%H%M%SZ')"
backup_dir="/root/niffler-ovh-bootstrap-$timestamp"
install -d -m 0700 "$backup_dir"

backup_file() {
    local path="$1"

    if [ -e "$path" ] || [ -L "$path" ]; then
        cp -a --parents "$path" "$backup_dir"
    fi
}

backup_file /etc/apt/sources.list
backup_file /etc/apt/sources.list.d
backup_file /etc/docker/daemon.json
backup_file /etc/fail2ban/jail.d
backup_file /etc/systemd/resolved.conf
backup_file /etc/systemd/resolved.conf.d
backup_file /etc/ufw

apt-get update
apt-get -y full-upgrade
apt-get install -y \
    ca-certificates \
    curl \
    dnsutils \
    fail2ban \
    git \
    gnupg \
    jq \
    lsof \
    rsync \
    unattended-upgrades \
    ufw \
    wireguard-tools

install -m 0755 -d /etc/apt/keyrings
curl -fsSL https://download.docker.com/linux/debian/gpg \
    -o /etc/apt/keyrings/docker.asc
chmod 0644 /etc/apt/keyrings/docker.asc

cat > /etc/apt/sources.list.d/docker.sources <<'EOF'
Types: deb
URIs: https://download.docker.com/linux/debian
Suites: trixie
Components: stable
Architectures: amd64
Signed-By: /etc/apt/keyrings/docker.asc
EOF

apt-get update
apt-get install -y \
    containerd.io \
    docker-buildx-plugin \
    docker-ce \
    docker-ce-cli \
    docker-compose-plugin

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

install -d -m 0755 /etc/systemd/resolved.conf.d
cat > /etc/systemd/resolved.conf.d/10-niffler-security.conf <<'EOF'
[Resolve]
LLMNR=no
MulticastDNS=no
EOF

cat > /etc/apt/apt.conf.d/20auto-upgrades <<'EOF'
APT::Periodic::Update-Package-Lists "1";
APT::Periodic::Unattended-Upgrade "1";
EOF

cat > /etc/sysctl.d/90-niffler-security.conf <<'EOF'
kernel.dmesg_restrict = 1
kernel.kptr_restrict = 2
net.ipv4.conf.all.accept_redirects = 0
net.ipv4.conf.default.accept_redirects = 0
net.ipv4.conf.all.send_redirects = 0
net.ipv4.conf.default.send_redirects = 0
net.ipv6.conf.all.accept_redirects = 0
net.ipv6.conf.default.accept_redirects = 0
EOF

sysctl --system >/dev/null
systemctl restart systemd-resolved
systemctl enable --now docker
systemctl restart docker
systemctl enable --now fail2ban
systemctl enable --now unattended-upgrades

ufw default deny incoming
ufw default allow outgoing
ufw allow OpenSSH
ufw logging low
ufw --force enable

install -d -o root -g root -m 0700 /opt/niffler-app
install -d -o root -g root -m 0750 \
    /opt/niffler-app/logs \
    /opt/niffler-app/logs/frontdoor \
    /opt/niffler-release \
    /opt/niffler-release/bin

docker run --rm hello-world >/dev/null

printf 'backup_dir=%s\n' "$backup_dir"
printf 'docker_version=%s\n' "$(docker version --format '{{.Server.Version}}')"
printf 'compose_version=%s\n' "$(docker compose version --short)"
printf 'ufw_status=%s\n' "$(ufw status | head -n 1)"
printf 'fail2ban_status=%s\n' "$(systemctl is-active fail2ban)"
printf 'docker_status=%s\n' "$(systemctl is-active docker)"
if [ -f /var/run/reboot-required ]; then
    printf 'reboot_required=yes\n'
else
    printf 'reboot_required=no\n'
fi
