#!/bin/bash

set -euo pipefail

MODE="${1:-run}"
CONFIG_FILE="${NIFFLER_MONITOR_CONFIG_FILE:-/etc/niffler-monitor/monitor.env}"
STATE_DIR="${NIFFLER_MONITOR_STATE_DIR:-/var/lib/niffler-monitor}"
RUNTIME_DIR="${NIFFLER_MONITOR_RUNTIME_DIR:-/run/niffler-monitor}"

declare -a SUMMARY_LINES=()
declare -a ALERT_LINES=()
declare -a UPDATE_CHECKS=()
declare -a UPDATE_STATES=()
declare -a UPDATE_COUNTS=()
declare -a TEMP_FILES=()
SUMMARY_ISSUE_COUNT=0

die() {
    echo "ERROR: $*" >&2
    exit 1
}

require_command() {
    command -v "$1" >/dev/null 2>&1 || die "required command is unavailable: $1"
}

cleanup() {
    local file_path

    [ "${#TEMP_FILES[@]}" -gt 0 ] || return
    for file_path in "${TEMP_FILES[@]}"; do
        if [ -f "$file_path" ] && [ ! -L "$file_path" ]; then
            rm -f -- "$file_path"
        fi
    done
}
trap cleanup EXIT

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

read_state_value() {
    local state_file="$1"
    local key="$2"

    awk -F= -v key="$key" '
        $1 == key {
            sub(/^[^=]*=/, "")
            print
            exit
        }
    ' "$state_file"
}

load_state() {
    local check_name="$1"
    local state_file="$STATE_DIR/$check_name.state"

    PREVIOUS_STATE="unknown"
    PREVIOUS_COUNT=0
    if [ ! -f "$state_file" ] || [ -L "$state_file" ]; then
        return
    fi

    PREVIOUS_STATE="$(read_state_value "$state_file" STATE)"
    PREVIOUS_COUNT="$(read_state_value "$state_file" FAILURE_COUNT)"
    if [[ ! "$PREVIOUS_STATE" =~ ^(ok|pending|failed|warning|critical)$ ]]; then
        PREVIOUS_STATE="unknown"
    fi
    if [[ ! "$PREVIOUS_COUNT" =~ ^[0-9]+$ ]]; then
        PREVIOUS_COUNT=0
    fi
}

queue_state_update() {
    UPDATE_CHECKS+=("$1")
    UPDATE_STATES+=("$2")
    UPDATE_COUNTS+=("$3")
}

apply_state_updates() {
    local index
    local check_name
    local temporary_file
    local state_file

    for index in "${!UPDATE_CHECKS[@]}"; do
        check_name="${UPDATE_CHECKS[$index]}"
        state_file="$STATE_DIR/$check_name.state"
        temporary_file="$STATE_DIR/.$check_name.state.$$"
        {
            printf 'STATE=%s\n' "${UPDATE_STATES[$index]}"
            printf 'FAILURE_COUNT=%s\n' "${UPDATE_COUNTS[$index]}"
            printf 'UPDATED_AT=%s\n' "$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
        } > "$temporary_file"
        chmod 0600 "$temporary_file"
        mv "$temporary_file" "$state_file"
    done
}

evaluate_binary_check() {
    local check_name="$1"
    local label="$2"
    local is_healthy="$3"
    local detail="$4"
    local next_count

    SUMMARY_LINES+=("${label}：$detail")
    if [ "$is_healthy" != "1" ]; then
        SUMMARY_ISSUE_COUNT=$((SUMMARY_ISSUE_COUNT + 1))
    fi
    if [ "$MODE" != "run" ]; then
        return
    fi

    load_state "$check_name"
    if [ "$is_healthy" = "1" ]; then
        if [ "$PREVIOUS_STATE" = "failed" ]; then
            ALERT_LINES+=("$label 已恢复正常。")
        fi
        queue_state_update "$check_name" ok 0
        return
    fi

    next_count=$((PREVIOUS_COUNT + 1))
    if [ "$next_count" -ge "$MONITOR_FAILURE_THRESHOLD" ]; then
        if [ "$PREVIOUS_STATE" != "failed" ]; then
            ALERT_LINES+=(
                "${label}连续 $MONITOR_FAILURE_THRESHOLD 次检查异常，可能已经影响正常使用。"
            )
        fi
        queue_state_update "$check_name" failed "$MONITOR_FAILURE_THRESHOLD"
    else
        queue_state_update "$check_name" pending "$next_count"
    fi
}

evaluate_disk_check() {
    local disk_path="$1"
    local usage_percent="$2"
    local available_kb="$3"
    local check_name
    local current_state
    local detail
    local available_size

    check_name="disk_$(printf '%s' "$disk_path" | tr -c 'A-Za-z0-9_.-' '_')"
    available_size="$(awk -v kb="$available_kb" '
        BEGIN {
            if (kb >= 1048576) {
                printf "%.1f GB", kb / 1048576
            } else {
                printf "%.0f MB", kb / 1024
            }
        }
    ')"
    if [ "$usage_percent" -ge "$MONITOR_DISK_CRITICAL_PERCENT" ]; then
        current_state="critical"
        detail="空间严重不足，已使用 ${usage_percent}%，剩余 $available_size"
    elif [ "$usage_percent" -ge "$MONITOR_DISK_WARNING_PERCENT" ]; then
        current_state="warning"
        detail="空间偏少，已使用 ${usage_percent}%，剩余 $available_size"
    else
        current_state="ok"
        detail="正常，已使用 ${usage_percent}%，剩余 $available_size"
    fi
    SUMMARY_LINES+=("${MONITOR_DISK_LABEL}：$detail")
    if [ "$current_state" != "ok" ]; then
        SUMMARY_ISSUE_COUNT=$((SUMMARY_ISSUE_COUNT + 1))
    fi
    if [ "$MODE" != "run" ]; then
        return
    fi

    load_state "$check_name"
    if [ "$PREVIOUS_STATE" != "$current_state" ]; then
        case "$current_state" in
            critical)
                ALERT_LINES+=(
                    "${MONITOR_DISK_LABEL}空间严重不足：已使用 ${usage_percent}%，剩余 $available_size。严重值为 ${MONITOR_DISK_CRITICAL_PERCENT}%，请立即清理文件或扩容。"
                )
                ;;
            warning)
                if [ "$PREVIOUS_STATE" = "critical" ]; then
                    ALERT_LINES+=(
                        "${MONITOR_DISK_LABEL}已脱离严重状态，但空间仍然偏少：已使用 ${usage_percent}%，剩余 $available_size。"
                    )
                else
                    ALERT_LINES+=(
                        "${MONITOR_DISK_LABEL}空间偏少：已使用 ${usage_percent}%，剩余 $available_size。预警值为 ${MONITOR_DISK_WARNING_PERCENT}%，建议尽快清理日志或扩容。"
                    )
                fi
                ;;
            ok)
                if [ "$PREVIOUS_STATE" != "unknown" ]; then
                    ALERT_LINES+=(
                        "${MONITOR_DISK_LABEL}空间已恢复正常：已使用 ${usage_percent}%，剩余 $available_size。"
                    )
                fi
                ;;
        esac
    fi
    queue_state_update "$check_name" "$current_state" 0
}

send_telegram_message() {
    local title="$1"
    shift
    local message
    local response_file
    local curl_config
    local http_code

    message="$(
        printf '%s\n' \
            "$title" \
            "服务器：$MONITOR_NODE_DISPLAY_NAME" \
            "$@" \
            "时间：$(TZ=Asia/Shanghai date '+%Y-%m-%d %H:%M:%S %Z')"
    )"
    response_file="$(mktemp "$RUNTIME_DIR/telegram-response.XXXXXX")"
    curl_config="$(mktemp "$RUNTIME_DIR/telegram-curl.XXXXXX")"
    TEMP_FILES+=("$response_file" "$curl_config")
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
}

write_report() {
    local report_directory
    local temporary_file

    if [ -z "$MONITOR_REPORT_FILE" ]; then
        return
    fi
    report_directory="$(dirname "$MONITOR_REPORT_FILE")"
    if [ ! -d "$report_directory" ]; then
        install -d -m 0700 "$report_directory"
    fi
    temporary_file="$(mktemp "$report_directory/.status.XXXXXX")"
    TEMP_FILES+=("$temporary_file")
    {
        printf '%s\n' "$MONITOR_NODE_DISPLAY_NAME"
        printf '%s\n' "${SUMMARY_LINES[@]}"
        if [ "$SUMMARY_ISSUE_COUNT" -eq 0 ]; then
            printf '结论：所有检查均正常\n'
        else
            printf '结论：发现 %s 项需要处理\n' "$SUMMARY_ISSUE_COUNT"
        fi
        printf '检查时间：%s\n' "$(TZ=Asia/Shanghai date '+%Y-%m-%d %H:%M:%S')"
    } > "$temporary_file"
    chmod 0644 "$temporary_file"
    mv "$temporary_file" "$MONITOR_REPORT_FILE"
}

if [ "$EUID" -ne 0 ]; then
    die "must run as root"
fi
if [ "$MODE" != "run" ] && [ "$MODE" != "test" ] && [ "$MODE" != "report" ]; then
    die "mode must be run, test or report"
fi

require_command curl
require_command df
require_command docker
require_command jq
require_command stat

validate_secure_file "$CONFIG_FILE" "monitor configuration"
set -a
# shellcheck disable=SC1090
source "$CONFIG_FILE"
set +a

: "${MONITOR_NODE_NAME:?}"
: "${MONITOR_DISK_PATH:?}"
: "${MONITOR_CONTAINERS:?}"

MONITOR_NODE_DISPLAY_NAME="${MONITOR_NODE_DISPLAY_NAME:-$MONITOR_NODE_NAME}"
MONITOR_DISK_LABEL="${MONITOR_DISK_LABEL:-系统盘}"
MONITOR_CONTAINER_LABELS="${MONITOR_CONTAINER_LABELS:-$MONITOR_CONTAINERS}"
MONITOR_DISK_WARNING_PERCENT="${MONITOR_DISK_WARNING_PERCENT:-80}"
MONITOR_DISK_CRITICAL_PERCENT="${MONITOR_DISK_CRITICAL_PERCENT:-90}"
MONITOR_FAILURE_THRESHOLD="${MONITOR_FAILURE_THRESHOLD:-3}"
MONITOR_HTTP_URL="${MONITOR_HTTP_URL:-}"
MONITOR_HTTP_LABEL="${MONITOR_HTTP_LABEL:-网站访问}"
MONITOR_REPORT_FILE="${MONITOR_REPORT_FILE:-$STATE_DIR/status.txt}"
MONITOR_POSTGRES_CONTAINER="${MONITOR_POSTGRES_CONTAINER:-}"
MONITOR_POSTGRES_ROLE="${MONITOR_POSTGRES_ROLE:-}"
MONITOR_POSTGRES_LABEL="${MONITOR_POSTGRES_LABEL:-数据库角色}"
MONITOR_POSTGRES_PORT="${MONITOR_POSTGRES_PORT:-5432}"
MONITOR_POSTGRES_MAX_REPLAY_LAG_BYTES="${MONITOR_POSTGRES_MAX_REPLAY_LAG_BYTES:-16777216}"
MONITOR_POSTGRES_LAG_IGNORE_SERVICE="${MONITOR_POSTGRES_LAG_IGNORE_SERVICE:-}"

if [[ ! "$MONITOR_NODE_NAME" =~ ^[A-Za-z0-9_.-]+$ ]]; then
    die "monitor node name is invalid"
fi
for numeric_value in \
    "$MONITOR_DISK_WARNING_PERCENT" \
    "$MONITOR_DISK_CRITICAL_PERCENT" \
    "$MONITOR_FAILURE_THRESHOLD"; do
    if [[ ! "$numeric_value" =~ ^[1-9][0-9]*$ ]]; then
        die "monitor thresholds must be positive integers"
    fi
done
if [ "$MONITOR_DISK_WARNING_PERCENT" -ge "$MONITOR_DISK_CRITICAL_PERCENT" ]; then
    die "disk warning threshold must be lower than critical threshold"
fi
if [ -n "$MONITOR_POSTGRES_CONTAINER" ]; then
    if [[ ! "$MONITOR_POSTGRES_CONTAINER" =~ ^[A-Za-z0-9_.-]+$ ]]; then
        die "PostgreSQL container name is invalid"
    fi
    if [ "$MONITOR_POSTGRES_ROLE" != "primary" ] && [ "$MONITOR_POSTGRES_ROLE" != "standby" ]; then
        die "PostgreSQL role must be primary or standby"
    fi
    if [[ ! "$MONITOR_POSTGRES_PORT" =~ ^[0-9]+$ ]] ||
        [ "$MONITOR_POSTGRES_PORT" -lt 1 ] || [ "$MONITOR_POSTGRES_PORT" -gt 65535 ]; then
        die "PostgreSQL port must be between 1 and 65535"
    fi
    if [[ ! "$MONITOR_POSTGRES_MAX_REPLAY_LAG_BYTES" =~ ^[0-9]+$ ]]; then
        die "PostgreSQL replay lag threshold must be a non-negative integer"
    fi
    if [ -n "$MONITOR_POSTGRES_LAG_IGNORE_SERVICE" ]; then
        if [[ ! "$MONITOR_POSTGRES_LAG_IGNORE_SERVICE" =~ ^[A-Za-z0-9_.@-]+\.service$ ]]; then
            die "PostgreSQL lag ignore service name is invalid"
        fi
        require_command systemctl
    fi
fi

if [ "$MODE" != "report" ]; then
    : "${MONITOR_TELEGRAM_CONFIG_FILE:?}"
    validate_secure_file "$MONITOR_TELEGRAM_CONFIG_FILE" "Telegram credential"
    set -a
    # shellcheck disable=SC1090
    source "$MONITOR_TELEGRAM_CONFIG_FILE"
    set +a
    : "${TELEGRAM_BOT_TOKEN:?}"
    : "${TELEGRAM_CHAT_ID:?}"
    if [[ ! "$TELEGRAM_BOT_TOKEN" =~ ^[0-9]+:[A-Za-z0-9_-]+$ ]]; then
        die "Telegram Bot Token format is invalid"
    fi
    if [[ ! "$TELEGRAM_CHAT_ID" =~ ^-?[0-9]+$ ]]; then
        die "Telegram Chat ID format is invalid"
    fi
fi

install -d -m 0700 "$STATE_DIR" "$RUNTIME_DIR"

read -r disk_usage disk_available_kb < <(
    df -Pk "$MONITOR_DISK_PATH" |
        awk 'NR == 2 {gsub(/%/, "", $5); print $5, $4}'
)
if [[ ! "$disk_usage" =~ ^[0-9]+$ ]] || [[ ! "$disk_available_kb" =~ ^[0-9]+$ ]]; then
    die "disk usage could not be determined"
fi
evaluate_disk_check "$MONITOR_DISK_PATH" "$disk_usage" "$disk_available_kb"

IFS=',' read -r -a container_names <<< "$MONITOR_CONTAINERS"
IFS=',' read -r -a container_labels <<< "$MONITOR_CONTAINER_LABELS"
if [ "${#container_names[@]}" -ne "${#container_labels[@]}" ]; then
    die "container names and labels must have the same item count"
fi
for container_index in "${!container_names[@]}"; do
    container_name="${container_names[$container_index]}"
    container_label="${container_labels[$container_index]}"
    container_name="${container_name//[[:space:]]/}"
    container_label="${container_label#"${container_label%%[![:space:]]*}"}"
    container_label="${container_label%"${container_label##*[![:space:]]}"}"
    if [[ ! "$container_name" =~ ^[A-Za-z0-9_.-]+$ ]]; then
        die "container name is invalid"
    fi
    if [ -z "$container_label" ]; then
        die "container label is empty"
    fi
    if container_state="$(
        docker inspect --format \
            '{{.State.Running}} {{if .State.Health}}{{.State.Health.Status}}{{else}}none{{end}}' \
            "$container_name" 2>/dev/null
    )"; then
        read -r container_running container_health <<< "$container_state"
        if [ "$container_running" = "true" ] &&
            { [ "$container_health" = "healthy" ] || [ "$container_health" = "none" ]; }; then
            evaluate_binary_check \
                "container_$container_name" \
                "$container_label" \
                1 \
                "运行正常"
        else
            evaluate_binary_check \
                "container_$container_name" \
                "$container_label" \
                0 \
                "未正常运行"
        fi
    else
        evaluate_binary_check \
            "container_$container_name" \
            "$container_label" \
            0 \
            "无法读取运行状态"
    fi
done

if [ -n "$MONITOR_POSTGRES_CONTAINER" ]; then
    if [ "$MONITOR_POSTGRES_ROLE" = "primary" ]; then
        if postgres_state="$(
            docker exec "$MONITOR_POSTGRES_CONTAINER" \
                psql -p "$MONITOR_POSTGRES_PORT" -U postgres -d postgres -X -Atqc \
                'select pg_is_in_recovery()' 2>/dev/null
        )" && [ "$postgres_state" = "f" ]; then
            evaluate_binary_check postgres_role "$MONITOR_POSTGRES_LABEL" 1 "可正常写入，角色正确"
        else
            evaluate_binary_check postgres_role "$MONITOR_POSTGRES_LABEL" 0 "角色异常或暂时无法确认主库状态"
        fi
    else
        postgres_lag_ignore_state="inactive"
        if [ -n "$MONITOR_POSTGRES_LAG_IGNORE_SERVICE" ]; then
            postgres_lag_ignore_state="$(
                systemctl show "$MONITOR_POSTGRES_LAG_IGNORE_SERVICE" \
                    --property ActiveState --value 2>/dev/null || printf 'unknown'
            )"
        fi
        postgres_state="$(
            docker exec "$MONITOR_POSTGRES_CONTAINER" \
                psql -p "$MONITOR_POSTGRES_PORT" -U postgres -d postgres -X -Atqc \
                "select pg_is_in_recovery(), coalesce((select status from pg_stat_wal_receiver limit 1), 'stopped'), coalesce((select pg_wal_lsn_diff(latest_end_lsn, pg_last_wal_replay_lsn())::bigint from pg_stat_wal_receiver limit 1), 0)" \
                2>/dev/null
        )" || postgres_state=""
        IFS='|' read -r postgres_recovery postgres_receiver postgres_lag <<< "$postgres_state"
        if [ "$postgres_recovery" = "t" ] && [ "$postgres_receiver" = "streaming" ] &&
            [[ "$postgres_lag" =~ ^[0-9]+$ ]]; then
            if [ "$postgres_lag" -le "$MONITOR_POSTGRES_MAX_REPLAY_LAG_BYTES" ]; then
                evaluate_binary_check postgres_role "$MONITOR_POSTGRES_LABEL" 1 "只读同步正常，待重放 ${postgres_lag} 字节"
            elif [ "$postgres_lag_ignore_state" = "active" ] ||
                [ "$postgres_lag_ignore_state" = "activating" ] ||
                [ "$postgres_lag_ignore_state" = "reloading" ]; then
                evaluate_binary_check postgres_role "$MONITOR_POSTGRES_LABEL" 1 "备份进行中，复制连接正常，待重放 ${postgres_lag} 字节"
            else
                evaluate_binary_check postgres_role "$MONITOR_POSTGRES_LABEL" 0 "复制延迟过高，待重放 ${postgres_lag} 字节"
            fi
        else
            evaluate_binary_check postgres_role "$MONITOR_POSTGRES_LABEL" 0 "复制中断、角色错误或暂时无法确认从库状态"
        fi
    fi
fi

if [ -n "$MONITOR_HTTP_URL" ]; then
    http_response_file="$(mktemp "$RUNTIME_DIR/http-response.XXXXXX")"
    TEMP_FILES+=("$http_response_file")
    if curl \
        --silent \
        --show-error \
        --fail \
        --output "$http_response_file" \
        --connect-timeout 5 \
        --max-time 10 \
        "$MONITOR_HTTP_URL" &&
        jq -e '.status == "ok"' "$http_response_file" >/dev/null; then
        evaluate_binary_check http_health "$MONITOR_HTTP_LABEL" 1 "可以正常访问"
    else
        evaluate_binary_check http_health "$MONITOR_HTTP_LABEL" 0 "暂时无法访问或返回异常"
    fi
fi

write_report

if [ "$MODE" = "report" ]; then
    cat "$MONITOR_REPORT_FILE"
    exit 0
fi

if [ "$MODE" = "test" ]; then
    send_telegram_message \
        '[Niffler 监控测试消息]' \
        '说明：这是人工触发的检查摘要，不代表刚发生故障。' \
        "${SUMMARY_LINES[@]}"
    echo "Telegram production monitor test delivered"
    exit 0
fi

if [ "${#ALERT_LINES[@]}" -gt 0 ]; then
    send_telegram_message \
        "[Niffler 需要处理：$MONITOR_NODE_DISPLAY_NAME]" \
        "${ALERT_LINES[@]}" \
        '如果不确定如何处理，请先保留这条消息并联系维护人员。'
fi
apply_state_updates
echo "Production monitor completed: ${#ALERT_LINES[@]} notification changes"
