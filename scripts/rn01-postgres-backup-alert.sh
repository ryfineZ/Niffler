#!/bin/bash

set -euo pipefail

MODE="${1:-failure}"
CONFIG_FILE="${NIFFLER_TELEGRAM_CONFIG_FILE:-/etc/niffler-backup/telegram.env}"
STATUS_FILE="${NIFFLER_BACKUP_STATUS_FILE:-/var/lib/niffler-backup/status.env}"
BACKUP_UNIT="niffler-postgres-backup.service"
SERVER_LABEL="${NIFFLER_BACKUP_SERVER_LABEL:-数据库服务器（rn01）}"
LOG_HOST="${NIFFLER_BACKUP_LOG_HOST:-rn01}"

die() {
    echo "ERROR: $*" >&2
    exit 1
}

require_command() {
    command -v "$1" >/dev/null 2>&1 || die "required command is unavailable: $1"
}

read_status_value() {
    local key="$1"

    if [ ! -f "$STATUS_FILE" ] || [ -L "$STATUS_FILE" ]; then
        printf 'unknown'
        return
    fi
    awk -F= -v key="$key" '
        $1 == key {
            sub(/^[^=]*=/, "")
            print
            found = 1
            exit
        }
        END {
            if (!found) {
                print "unknown"
            }
        }
    ' "$STATUS_FILE"
}

if [ "$EUID" -ne 0 ]; then
    die "must run as root"
fi
if [ "$MODE" != "failure" ] && [ "$MODE" != "success" ] && [ "$MODE" != "test" ]; then
    die "mode must be failure, success or test"
fi

require_command curl
require_command jq
require_command numfmt
require_command stat
require_command systemctl

if [ ! -f "$CONFIG_FILE" ] || [ -L "$CONFIG_FILE" ]; then
    die "Telegram credential file is missing or unsafe"
fi
if [ "$(stat -c '%a' "$CONFIG_FILE")" != "600" ]; then
    die "Telegram credential file must use mode 600"
fi

set -a
# shellcheck disable=SC1090
source "$CONFIG_FILE"
set +a

: "${TELEGRAM_BOT_TOKEN:?}"
: "${TELEGRAM_CHAT_ID:?}"

if [[ ! "$TELEGRAM_BOT_TOKEN" =~ ^[0-9]+:[A-Za-z0-9_-]+$ ]]; then
    die "Telegram Bot Token format is invalid"
fi
if [[ ! "$TELEGRAM_CHAT_ID" =~ ^-?[0-9]+$ ]]; then
    die "Telegram Chat ID format is invalid"
fi

updated_at="$(TZ=Asia/Shanghai date '+%Y-%m-%d %H:%M:%S %Z')"

if [ "$MODE" = "test" ]; then
    message="$(
        printf '%s\n' \
            '[Niffler 备份测试消息]' \
            '说明：这是人工触发的测试，不代表备份刚刚执行。' \
            "服务器：$SERVER_LABEL" \
            '结果：Telegram 通知可以正常送达。' \
            "时间：$updated_at"
)"
elif [ "$MODE" = "success" ]; then
    backup_bytes="$(read_status_value BACKUP_BYTES)"
    if [[ "$backup_bytes" =~ ^[0-9]+$ ]]; then
        backup_size="$(numfmt --to=iec-i --suffix=B --format='%.1f' "$backup_bytes")"
    else
        backup_size="大小暂时无法读取"
    fi
    message="$(
        printf '%s\n' \
            '[Niffler 数据库备份完成]' \
            "服务器：$SERVER_LABEL" \
            '结果：备份已经上传到 Cloudflare R2，并通过完整性校验。' \
            "备份大小：$backup_size" \
            '用途：数据库发生故障时，可以使用这份备份恢复数据。' \
            "时间：$updated_at"
    )"
else
    unit_result="$(systemctl show "$BACKUP_UNIT" --property=Result --value)"
    exit_status="$(systemctl show "$BACKUP_UNIT" --property=ExecMainStatus --value)"
    backup_status="$(read_status_value STATUS)"
    message="$(
        printf '%s\n' \
            '[Niffler 需要处理：数据库备份失败]' \
            "服务器：$SERVER_LABEL" \
            '结果：本次没有生成可确认使用的数据库备份。' \
            '影响：如果此时数据库发生故障，无法依靠本次任务恢复数据。' \
            "错误参考：任务状态 ${backup_status:-unknown}，结果 ${unit_result:-unknown}，代码 ${exit_status:-unknown}" \
            "处理建议：请尽快检查 $LOG_HOST 的备份任务日志并重新执行备份。" \
            "时间：$updated_at" \
            '日志命令：journalctl -u niffler-postgres-backup.service'
    )"
fi

response_file="$(mktemp /tmp/niffler-telegram-response.XXXXXX)"
curl_config="$(mktemp /tmp/niffler-telegram-curl.XXXXXX)"
cleanup() {
    rm -f -- "$response_file" "$curl_config"
}
trap cleanup EXIT
chmod 0600 "$response_file" "$curl_config"

printf 'url = "https://api.telegram.org/bot%s/sendMessage"\n' \
    "$TELEGRAM_BOT_TOKEN" > "$curl_config"

http_code="$(
    curl \
        --silent \
        --show-error \
        --output "$response_file" \
        --write-out '%{http_code}' \
        --config "$curl_config" \
        --request POST \
        --connect-timeout 10 \
        --max-time 20 \
        --retry 2 \
        --retry-all-errors \
        --data-urlencode "chat_id=$TELEGRAM_CHAT_ID" \
        --data-urlencode "text=$message"
)"

if [ "$http_code" != "200" ]; then
    die "Telegram API returned HTTP $http_code"
fi
if ! jq -e '.ok == true' "$response_file" >/dev/null; then
    die "Telegram API did not accept the message"
fi

echo "Telegram backup alert delivered ($MODE)"
