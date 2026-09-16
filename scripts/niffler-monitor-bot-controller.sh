#!/bin/bash

set -euo pipefail

MODE="${1:-run}"
TELEGRAM_CONFIG_FILE="${NIFFLER_BOT_TELEGRAM_CONFIG_FILE:-/etc/niffler-monitor/telegram.env}"
LOCAL_CONFIG_FILE="${NIFFLER_MONITOR_CONFIG_FILE:-/etc/niffler-monitor/monitor.env}"
LOCAL_SETTINGS_SCRIPT="${NIFFLER_MONITOR_SETTINGS_SCRIPT:-/usr/local/sbin/niffler-monitor-settings}"
MONITOR_SCRIPT="${NIFFLER_MONITOR_SCRIPT:-/usr/local/sbin/niffler-production-monitor}"
STATE_DIR="${NIFFLER_BOT_STATE_DIR:-/var/lib/niffler-monitor-bot}"
OFFSET_FILE="$STATE_DIR/update-offset"
LOCK_FILE="$STATE_DIR/controller.lock"
SSH_CONFIG="${NIFFLER_MONITOR_SSH_CONFIG:-/etc/niffler-monitor-bot/ssh_config}"
HD0526_ALIAS="${NIFFLER_MONITOR_HD0526_ALIAS:-hd0526-monitor}"
DMIT_ALIAS="${NIFFLER_MONITOR_DMIT_ALIAS:-dmit-monitor}"
declare -a TEMP_FILES=()

die() {
    echo "ERROR: $*" >&2
    exit 1
}

cleanup() {
    local file_path

    [ "${#TEMP_FILES[@]}" -gt 0 ] || return
    for file_path in "${TEMP_FILES[@]}"; do
        [ -f "$file_path" ] && [ ! -L "$file_path" ] && rm -f -- "$file_path"
    done
}
trap cleanup EXIT

require_command() {
    command -v "$1" >/dev/null 2>&1 || die "required command is unavailable: $1"
}

validate_secure_file() {
    local file_path="$1"
    local description="$2"

    if [ ! -f "$file_path" ] || [ -L "$file_path" ]; then
        die "$description file is missing or unsafe"
    fi
    if [ "$(stat -c '%a' "$file_path")" != "600" ]; then
        die "$description file must use mode 600"
    fi
}

send_message() {
    local chat_id="$1"
    local message="$2"
    local response_file
    local curl_config
    local http_code

    response_file="$(mktemp "$STATE_DIR/response.XXXXXX")"
    curl_config="$(mktemp "$STATE_DIR/curl.XXXXXX")"
    trap 'rm -f -- "${response_file:-}" "${curl_config:-}"' RETURN
    chmod 0600 "$response_file" "$curl_config"
    printf 'url = "https://api.telegram.org/bot%s/sendMessage"\n' \
        "$TELEGRAM_BOT_TOKEN" > "$curl_config"
    http_code="$(
        curl --silent --show-error --output "$response_file" --write-out '%{http_code}' \
            --config "$curl_config" --request POST --connect-timeout 10 --max-time 20 \
            --retry 2 --retry-all-errors \
            --data-urlencode "chat_id=$chat_id" --data-urlencode "text=$message"
    )"
    [ "$http_code" = "200" ] && jq -e '.ok == true' "$response_file" >/dev/null
}

get_updates() {
    local offset="${1:-}"
    local response_file
    local curl_config
    local query="timeout=0&limit=100"

    if [ -n "$offset" ]; then
        query="${query}&offset=${offset}"
    fi
    response_file="$(mktemp "$STATE_DIR/updates.XXXXXX")"
    curl_config="$(mktemp "$STATE_DIR/curl.XXXXXX")"
    TEMP_FILES+=("$response_file" "$curl_config")
    chmod 0600 "$response_file" "$curl_config"
    printf 'url = "https://api.telegram.org/bot%s/getUpdates?%s"\n' \
        "$TELEGRAM_BOT_TOKEN" "$query" > "$curl_config"
    curl --silent --show-error --output "$response_file" --write-out '%{http_code}' \
        --config "$curl_config" --request GET --connect-timeout 10 --max-time 20 |
        grep -qx '200' || die "Telegram getUpdates failed"
    jq -e '.ok == true' "$response_file" >/dev/null || die "Telegram getUpdates rejected"
    cat "$response_file"
}

set_bot_commands() {
    local response_file
    local curl_config
    local http_code
    local commands='[{"command":"status","description":"查看三台服务器状态"},{"command":"settings","description":"查看当前监控阈值"},{"command":"set_disk_warning","description":"设置磁盘预警值"},{"command":"set_disk_critical","description":"设置磁盘严重值"},{"command":"set_failures","description":"设置连续失败次数"},{"command":"help","description":"查看命令帮助"}]'

    response_file="$(mktemp "$STATE_DIR/commands-response.XXXXXX")"
    curl_config="$(mktemp "$STATE_DIR/commands-curl.XXXXXX")"
    TEMP_FILES+=("$response_file" "$curl_config")
    chmod 0600 "$response_file" "$curl_config"
    printf 'url = "https://api.telegram.org/bot%s/setMyCommands"\n' \
        "$TELEGRAM_BOT_TOKEN" > "$curl_config"
    http_code="$(
        curl --silent --show-error --output "$response_file" --write-out '%{http_code}' \
            --config "$curl_config" --request POST --connect-timeout 10 --max-time 20 \
            --data-urlencode "commands=$commands"
    )"
    [ "$http_code" = "200" ] && jq -e '.ok == true' "$response_file" >/dev/null
}

write_offset() {
    local next_offset="$1"
    local temporary_file

    temporary_file="$(mktemp "$STATE_DIR/.offset.XXXXXX")"
    printf '%s\n' "$next_offset" > "$temporary_file"
    chmod 0600 "$temporary_file"
    mv "$temporary_file" "$OFFSET_FILE"
}

remote_alias_for_target() {
    case "$1" in
        hd0526) printf '%s' "$HD0526_ALIAS" ;;
        dmit) printf '%s' "$DMIT_ALIAS" ;;
        *) die "target does not use remote SSH" ;;
    esac
}

settings_command() {
    local target="$1"
    local remote_command
    shift

    if [ "$target" = "rn01" ]; then
        "$LOCAL_SETTINGS_SCRIPT" "$@"
    else
        if [ "${1:-}" = "show" ] && [ "$#" -eq 1 ]; then
            remote_command="settings"
        else
            remote_command="$*"
        fi
        ssh -F "$SSH_CONFIG" "$(remote_alias_for_target "$target")" "$remote_command"
    fi
}

settings_text() {
    local target="$1"
    local values

    values="$(settings_command "$target" show)"
    printf '%s\n' "$values" | awk -F= -v target="$target" '
        $1 == "disk_warning" { warning = $2 }
        $1 == "disk_critical" { critical = $2 }
        $1 == "failures" { failures = $2 }
        END {
            printf "%s：磁盘空间达到 %s%% 时提醒，达到 %s%% 时视为严重；服务连续 %s 次检查失败后提醒。\n",
                target, warning, critical, failures
        }
    '
}

status_text() {
    "$MONITOR_SCRIPT" report
    ssh -F "$SSH_CONFIG" "$HD0526_ALIAS" status
    ssh -F "$SSH_CONFIG" "$DMIT_ALIAS" status
}

help_text() {
    cat <<'EOF'
可用命令：
/status - 查看三台服务器当前状态
/settings - 查看当前监控阈值
/set_disk_warning 85 - 设置磁盘预警值，同时修改三台服务器
/set_disk_critical 92 - 设置磁盘严重值，同时修改三台服务器
/set_failures 3 - 设置连续失败次数，同时修改三台服务器

只修改一台服务器时，在数字后加 rn01、hd0526 或 dmit，例如：
/set_disk_warning 85 dmit
EOF
}

validate_target() {
    [ "$1" = "all" ] || [ "$1" = "rn01" ] || [ "$1" = "hd0526" ] || [ "$1" = "dmit" ] ||
        die "target must be all, rn01, hd0526 or dmit"
}

set_setting() {
    local field="$1"
    local value="$2"
    local target="$3"
    local current_target
    local failed_target=""
    local label
    local -a targets=()
    local -a applied_targets=()
    local -A previous_values=()

    case "$field" in
        disk_warning) label="磁盘预警值" ;;
        disk_critical) label="磁盘严重值" ;;
        failures) label="连续失败次数" ;;
        *) return 1 ;;
    esac
    validate_target "$target"
    if [ "$target" = "all" ]; then
        targets=(hd0526 dmit rn01)
    else
        targets=("$target")
    fi
    for current_target in "${targets[@]}"; do
        settings_command "$current_target" validate "$field" "$value"
        previous_values["$current_target"]="$(settings_command "$current_target" get "$field")"
    done
    for current_target in "${targets[@]}"; do
        if settings_command "$current_target" set "$field" "$value" >/dev/null; then
            applied_targets+=("$current_target")
        else
            failed_target="$current_target"
            break
        fi
    done
    if [ -n "$failed_target" ]; then
        for current_target in "${applied_targets[@]}"; do
            settings_command "$current_target" set "$field" "${previous_values[$current_target]}" >/dev/null || true
        done
        return 1
    fi
    if [ "$field" = "failures" ]; then
        printf '%s已修改为 %s 次。\n' "$label" "$value"
        printf '连续 %s 次检查失败后发送提醒。\n' "$value"
    else
        printf '%s已修改为 %s%%。\n' "$label" "$value"
    fi
    printf '下一次监控检查时生效。'
}

process_update() {
    local update="$1"
    local update_id
    local chat_id
    local text
    local command
    local argument
    local target
    local extra
    local reply

    update_id="$(jq -r '.update_id' <<< "$update")"
    chat_id="$(jq -r '.message.chat.id // empty' <<< "$update")"
    text="$(jq -r '.message.text // empty' <<< "$update")"
    [ -n "$chat_id" ] && [ -n "$text" ] || return 0
    if [ "$chat_id" != "$TELEGRAM_CHAT_ID" ]; then
        return 0
    fi
    read -r command argument target extra <<< "$text"
    command="${command%%@*}"
    target="${target:-all}"
    case "$command" in
        /help|/start)
            reply="$(help_text)"
            ;;
        /status)
            [ -z "${argument:-}" ] || return 0
            reply="$(status_text)"
            ;;
        /settings)
            [ -z "${argument:-}" ] || return 0
            reply="$(settings_text rn01; settings_text hd0526; settings_text dmit)"
            ;;
        /set_disk_warning|/set_disk_critical|/set_failures)
            [ -n "${argument:-}" ] && [ -z "${extra:-}" ] || return 0
            case "$command" in
                /set_disk_warning) field="disk_warning" ;;
                /set_disk_critical) field="disk_critical" ;;
                /set_failures) field="failures" ;;
            esac
            if reply="$(set_setting "$field" "$argument" "$target" 2>&1)"; then
                :
            else
                reply="设置没有生效：$reply"
            fi
            ;;
        *) return 0 ;;
    esac
    send_message "$chat_id" "$reply" || logger -t niffler-monitor-bot "failed to reply update $update_id"
}

validate_config() {
    validate_secure_file "$TELEGRAM_CONFIG_FILE" "Telegram credential"
    validate_secure_file "$LOCAL_CONFIG_FILE" "local monitor configuration"
    set -a
    # shellcheck disable=SC1090
    source "$TELEGRAM_CONFIG_FILE"
    set +a
    : "${TELEGRAM_BOT_TOKEN:?}"
    : "${TELEGRAM_CHAT_ID:?}"
    [[ "$TELEGRAM_BOT_TOKEN" =~ ^[0-9]+:[A-Za-z0-9_-]+$ ]] ||
        die "Telegram Bot Token format is invalid"
    [[ "$TELEGRAM_CHAT_ID" =~ ^-?[0-9]+$ ]] ||
        die "Telegram Chat ID format is invalid"
}

if [ "$EUID" -ne 0 ]; then
    die "must run as root"
fi
if [ "$MODE" != "run" ] && [ "$MODE" != "setup" ]; then
    die "mode must be run or setup"
fi

require_command awk
require_command base64
require_command curl
require_command flock
require_command jq
require_command logger
require_command ssh
require_command stat
mkdir -p "$STATE_DIR"
chmod 0700 "$STATE_DIR"
exec 9>"$LOCK_FILE"
flock -n 9 || exit 0
validate_config

if [ "$MODE" = "setup" ]; then
    set_bot_commands || die "Telegram command registration failed"
    updates="$(get_updates)"
    latest_update_id="$(jq -r '[.result[].update_id] | max // empty' <<< "$updates")"
    if [ -n "$latest_update_id" ]; then
        write_offset "$((latest_update_id + 1))"
    else
        write_offset 0
    fi
    send_message "$TELEGRAM_CHAT_ID" "$(help_text)"
    exit 0
fi

offset=""
if [ -f "$OFFSET_FILE" ]; then
    offset="$(cat "$OFFSET_FILE")"
fi
updates="$(get_updates "$offset")"
while IFS= read -r encoded_update; do
    [ -n "$encoded_update" ] || continue
    update="$(printf '%s' "$encoded_update" | base64 --decode)"
    update_id="$(jq -r '.update_id' <<< "$update")"
    process_update "$update" || logger -t niffler-monitor-bot "failed to process update $update_id"
    write_offset "$((update_id + 1))"
done < <(jq -r '.result[] | @base64' <<< "$updates")
