#!/bin/bash

set -euo pipefail

env_file="${1:-}"
image_tag="${2:-}"

if [ -z "$env_file" ] || [ -z "$image_tag" ]; then
    echo "Usage: prepare-ovh-frontdoor-env.sh <env-file> <image-tag>" >&2
    exit 1
fi
if [ ! -f "$env_file" ] || [ -L "$env_file" ]; then
    echo "ERROR: environment file is missing or unsafe" >&2
    exit 1
fi
if [[ ! "$image_tag" =~ ^niffler-app:[0-9a-f]{40}$ ]]; then
    echo "ERROR: image tag must identify an exact Niffler commit" >&2
    exit 1
fi

temporary_file="$(mktemp "${env_file}.XXXXXX")"
trap 'rm -f -- "$temporary_file"' EXIT

awk -v image_tag="$image_tag" '
    BEGIN {
        image_written = 0
        port_written = 0
    }
    /^APP_IMAGE=/ {
        print "APP_IMAGE=" image_tag
        image_written = 1
        next
    }
    /^APP_PORT=/ {
        print "APP_PORT=18084"
        port_written = 1
        next
    }
    {
        gsub(/192\.129\.155\.207/, "10.71.0.1")
        print
    }
    END {
        if (!image_written) {
            print "APP_IMAGE=" image_tag
        }
        if (!port_written) {
            print "APP_PORT=18084"
        }
    }
' "$env_file" > "$temporary_file"

chmod 0600 "$temporary_file"
mv "$temporary_file" "$env_file"
trap - EXIT

if grep -q '192\.129\.155\.207' "$env_file"; then
    echo "ERROR: rn01 public address remains in OVH environment" >&2
    exit 1
fi
for key in \
    AETHER_DATABASE_URL \
    AETHER_GATEWAY_DATA_POSTGRES_URL \
    DATABASE_URL \
    AETHER_GATEWAY_DATA_REDIS_URL \
    AETHER_RUNTIME_REDIS_URL \
    REDIS_URL; do
    if ! grep -Eq "^${key}=.*10\\.71\\.0\\.1" "$env_file"; then
        echo "ERROR: $key does not use the WireGuard address" >&2
        exit 1
    fi
done
if ! grep -Eq '^AETHER_GATEWAY_DATA_POSTGRES_REQUIRE_SSL=true$' "$env_file"; then
    echo "ERROR: PostgreSQL TLS requirement is missing" >&2
    exit 1
fi

echo "ovh-frontdoor-env=ready"
