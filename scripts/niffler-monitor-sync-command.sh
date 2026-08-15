#!/bin/bash

set -euo pipefail

SETTINGS_SCRIPT="/usr/local/sbin/niffler-monitor-settings"
CONFIG_FILE="/var/lib/niffler-monitor-control/monitor.env"
STATUS_FILE="/var/lib/niffler-monitor-control/status.txt"
ORIGINAL_COMMAND="${SSH_ORIGINAL_COMMAND:-}"

export NIFFLER_MONITOR_CONFIG_FILE="$CONFIG_FILE"

case "$ORIGINAL_COMMAND" in
    status)
        if [ -f "$STATUS_FILE" ] && [ ! -L "$STATUS_FILE" ]; then
            cat "$STATUS_FILE"
        else
            echo "远程监控服务器"
            echo "状态：还没有生成监控结果，请稍后重试。"
        fi
        ;;
    settings)
        exec "$SETTINGS_SCRIPT" show
        ;;
    "validate disk_warning "*|"validate disk_critical "*|"validate failures "*)
        read -r action field value extra <<< "$ORIGINAL_COMMAND"
        [ -z "${extra:-}" ] || exit 64
        exec "$SETTINGS_SCRIPT" "$action" "$field" "$value"
        ;;
    "set disk_warning "*|"set disk_critical "*|"set failures "*)
        read -r action field value extra <<< "$ORIGINAL_COMMAND"
        [ -z "${extra:-}" ] || exit 64
        exec "$SETTINGS_SCRIPT" "$action" "$field" "$value"
        ;;
    "get disk_warning"|"get disk_critical"|"get failures")
        read -r action field <<< "$ORIGINAL_COMMAND"
        exec "$SETTINGS_SCRIPT" "$action" "$field"
        ;;
    *)
        echo "This SSH key can only read Niffler monitor status or change monitor thresholds." >&2
        exit 64
        ;;
esac
