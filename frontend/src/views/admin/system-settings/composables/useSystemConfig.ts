import { ref, computed } from 'vue'
import { useToast } from '@/composables/useToast'
import { adminApi } from '@/api/admin'
import { log } from '@/utils/logger'
import { useSiteInfo } from '@/composables/useSiteInfo'
import { useI18n } from 'vue-i18n'

export type ContentModerationLevel = 'off' | 'latest_user_input' | 'all_user_inputs' | 'full_request'
export type ContentModerationTargetKind = 'provider' | 'upstream_service' | 'upstream_account'

export interface ContentModerationTargetConfig {
  kind: ContentModerationTargetKind
  id: string
}

export interface ContentModerationAccountProtectionConfig {
  enabled: boolean
  level: ContentModerationLevel
  api_keys: string[]
  api_keys_clear: boolean
  api_key_count: number
  api_key_masks: string[]
  base_url: string
  model: string
  timeout_ms: number
  input_price_per_1m: number
  output_price_per_1m: number
  evidence_retention_days: number
  targets: ContentModerationTargetConfig[]
}

export interface SystemConfig {
  // 站点信息
  site_name: string
  site_subtitle: string
  contact_us_format: 'markdown' | 'html'
  contact_us_content: string
  // 网络代理
  system_proxy_node_id: string | null
  // 基础配置
  default_user_initial_gift_usd: number
  rate_limit_per_minute: number
  enable_registration: boolean
  password_policy_level: string
  turnstile_enabled: boolean
  turnstile_site_key: string | null
  turnstile_secret_key: string
  turnstile_secret_key_is_set: boolean
  turnstile_allowed_hostnames: string[]
  referral_enabled: boolean
  referral_reward_mode: string
  referral_recharge_percent: number
  referral_headcount_amount_usd: number
  referral_headcount_trigger: string
  registration_privacy_policy_enabled: boolean
  registration_privacy_policy_format: string
  registration_privacy_policy_content: string
  registration_privacy_policy_version: string
  // 独立余额 Key 过期管理
  auto_delete_expired_keys: boolean
  // 格式转换
  enable_format_conversion: boolean
  // Codex OAuth 身份收敛
  codex_oauth_identity_convergence_enabled: boolean
  // 同步生图心跳
  enable_openai_image_sync_heartbeat: boolean
  // 请求记录
  request_record_level: string
  max_request_body_size: number
  max_response_body_size: number
  sensitive_headers: string[]
  // 内容审查 / 账号保护
  content_moderation_account_protection: ContentModerationAccountProtectionConfig
  // 请求记录清理
  enable_auto_cleanup: boolean
  detail_log_retention_days: number
  compressed_log_retention_days: number
  header_retention_days: number
  log_retention_days: number
  cleanup_batch_size: number
  audit_log_retention_days: number
  request_candidates_retention_days: number
  request_candidates_cleanup_batch_size: number
  proxy_node_metrics_1m_retention_days: number
  proxy_node_metrics_1h_retention_days: number
  proxy_node_metrics_cleanup_batch_size: number
  // 定时任务
  enable_provider_checkin: boolean
  provider_checkin_time: string
  enable_oauth_token_refresh: boolean
}

const CONFIG_KEYS = [
  // 站点信息
  'site_name',
  'site_subtitle',
  'contact_us_format',
  'contact_us_content',
  // 网络代理
  'system_proxy_node_id',
  // 基础配置
  'default_user_initial_gift_usd',
  'rate_limit_per_minute',
  'enable_registration',
  'password_policy_level',
  'turnstile_enabled',
  'turnstile_site_key',
  'turnstile_secret_key',
  'turnstile_allowed_hostnames',
  'referral_enabled',
  'referral_reward_mode',
  'referral_recharge_percent',
  'referral_headcount_amount_usd',
  'referral_headcount_trigger',
  'registration_privacy_policy_enabled',
  'registration_privacy_policy_format',
  'registration_privacy_policy_content',
  'registration_privacy_policy_version',
  // 独立余额 Key 过期管理
  'auto_delete_expired_keys',
  // 格式转换
  'enable_format_conversion',
  // Codex OAuth 身份收敛
  'codex_oauth_identity_convergence_enabled',
  // 同步生图心跳
  'enable_openai_image_sync_heartbeat',
  // 请求记录
  'request_record_level',
  'max_request_body_size',
  'max_response_body_size',
  'sensitive_headers',
  // 内容审查 / 账号保护
  'content_moderation_account_protection',
  // 请求记录清理
  'enable_auto_cleanup',
  'detail_log_retention_days',
  'compressed_log_retention_days',
  'header_retention_days',
  'log_retention_days',
  'cleanup_batch_size',
  'audit_log_retention_days',
  'request_candidates_retention_days',
  'request_candidates_cleanup_batch_size',
  'proxy_node_metrics_1m_retention_days',
  'proxy_node_metrics_1h_retention_days',
  'proxy_node_metrics_cleanup_batch_size',
  // 定时任务
  'enable_provider_checkin',
  'provider_checkin_time',
  'enable_oauth_token_refresh',
]

function createDefaultContentModerationConfig(): ContentModerationAccountProtectionConfig {
  return {
    enabled: false,
    level: 'all_user_inputs',
    api_keys: [],
    api_keys_clear: false,
    api_key_count: 0,
    api_key_masks: [],
    base_url: 'https://api.openai.com/v1',
    model: 'omni-moderation-latest',
    timeout_ms: 3000,
    input_price_per_1m: 0,
    output_price_per_1m: 0,
    evidence_retention_days: 30,
    targets: [],
  }
}

function numberOrDefault(value: unknown, fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value) ? value : fallback
}

function stringOrDefault(value: unknown, fallback: string): string {
  return typeof value === 'string' && value.trim() ? value.trim() : fallback
}

function normalizeContentModerationLevel(value: unknown): ContentModerationLevel {
  if (
    value === 'off' ||
    value === 'latest_user_input' ||
    value === 'all_user_inputs' ||
    value === 'full_request'
  ) {
    return value
  }
  return 'all_user_inputs'
}

function normalizeContentModerationTargets(value: unknown): ContentModerationTargetConfig[] {
  if (!Array.isArray(value)) return []
  return value.flatMap((item) => {
    if (!item || typeof item !== 'object') return []
    const record = item as Record<string, unknown>
    const kind = record.kind
    const id = typeof record.id === 'string' ? record.id.trim() : ''
    if (
      !id ||
      (kind !== 'provider' && kind !== 'upstream_service' && kind !== 'upstream_account')
    ) {
      return []
    }
    return [{ kind, id }]
  })
}

function normalizeContentModerationApiKeys(value: unknown): string[] {
  if (!Array.isArray(value)) return []
  const seen = new Set<string>()
  return value.flatMap((item) => {
    if (typeof item !== 'string') return []
    const key = item.trim()
    if (!key || seen.has(key)) return []
    seen.add(key)
    return [key]
  })
}

function normalizeContentModerationApiKeyMasks(value: unknown): string[] {
  if (!Array.isArray(value)) return []
  return value.flatMap((item) => {
    if (typeof item !== 'string') return []
    const mask = item.trim()
    return mask ? [mask] : []
  })
}

function normalizeContentModerationConfig(value: unknown): ContentModerationAccountProtectionConfig {
  const defaults = createDefaultContentModerationConfig()
  if (!value || typeof value !== 'object') return defaults
  const record = value as Record<string, unknown>
  const apiKeys = normalizeContentModerationApiKeys(record.api_keys)
  const apiKeyMasks = normalizeContentModerationApiKeyMasks(record.api_key_masks)
  return {
    enabled: typeof record.enabled === 'boolean' ? record.enabled : defaults.enabled,
    level: normalizeContentModerationLevel(record.level),
    api_keys: apiKeys,
    api_keys_clear: typeof record.api_keys_clear === 'boolean' ? record.api_keys_clear : false,
    api_key_count: Math.max(
      apiKeys.length,
      Math.max(0, Math.round(numberOrDefault(record.api_key_count, defaults.api_key_count))),
    ),
    api_key_masks: apiKeyMasks,
    base_url: stringOrDefault(record.base_url, defaults.base_url),
    model: stringOrDefault(record.model, defaults.model),
    timeout_ms: Math.max(500, Math.min(60000, Math.round(numberOrDefault(record.timeout_ms, defaults.timeout_ms)))),
    input_price_per_1m: Math.max(0, numberOrDefault(record.input_price_per_1m, defaults.input_price_per_1m)),
    output_price_per_1m: Math.max(0, numberOrDefault(record.output_price_per_1m, defaults.output_price_per_1m)),
    evidence_retention_days: Math.max(
      1,
      Math.min(365, Math.round(numberOrDefault(record.evidence_retention_days, defaults.evidence_retention_days))),
    ),
    targets: normalizeContentModerationTargets(record.targets),
  }
}

function createDefaultConfig(): SystemConfig {
  return {
    // 站点信息
    site_name: 'Niffler',
    site_subtitle: 'AI Gateway',
    contact_us_format: 'markdown',
    contact_us_content: '',
    // 网络代理
    system_proxy_node_id: null,
    // 基础配置
    default_user_initial_gift_usd: 10.0,
    rate_limit_per_minute: 0,
    enable_registration: false,
    password_policy_level: 'weak',
    turnstile_enabled: false,
    turnstile_site_key: null,
    turnstile_secret_key: '',
    turnstile_secret_key_is_set: false,
    turnstile_allowed_hostnames: [],
    referral_enabled: false,
    referral_reward_mode: 'percent',
    referral_recharge_percent: 5,
    referral_headcount_amount_usd: 0,
    referral_headcount_trigger: 'registration',
    registration_privacy_policy_enabled: false,
    registration_privacy_policy_format: 'markdown',
    registration_privacy_policy_content: '',
    registration_privacy_policy_version: '1',
    // 独立余额 Key 过期管理
    auto_delete_expired_keys: false,
    // 格式转换
    enable_format_conversion: false,
    // Codex OAuth 身份收敛
    codex_oauth_identity_convergence_enabled: false,
    // 同步生图心跳
    enable_openai_image_sync_heartbeat: true,
    // 请求记录
    request_record_level: 'basic',
    max_request_body_size: 262144,
    max_response_body_size: 262144,
    sensitive_headers: [
      'authorization',
      'x-api-key',
      'api-key',
      'cookie',
      'set-cookie',
      'x-codex-installation-id',
      'session-id',
      'session_id',
      'thread-id',
      'x-client-request-id',
      'x-codex-window-id',
      'x-codex-turn-metadata',
    ],
    // 内容审查 / 账号保护
    content_moderation_account_protection: createDefaultContentModerationConfig(),
    // 请求记录清理
    enable_auto_cleanup: true,
    detail_log_retention_days: 1,
    compressed_log_retention_days: 2,
    header_retention_days: 30,
    log_retention_days: 365,
    cleanup_batch_size: 1000,
    audit_log_retention_days: 30,
    request_candidates_retention_days: 30,
    request_candidates_cleanup_batch_size: 5000,
    proxy_node_metrics_1m_retention_days: 30,
    proxy_node_metrics_1h_retention_days: 180,
    proxy_node_metrics_cleanup_batch_size: 5000,
    // 定时任务
    enable_provider_checkin: true,
    provider_checkin_time: '01:05',
    enable_oauth_token_refresh: true,
  }
}

export function useSystemConfig() {
  const { t } = useI18n()
  const { success, error } = useToast()
  const { refreshSiteInfo } = useSiteInfo()

  const systemConfig = ref<SystemConfig>(createDefaultConfig())
  const originalConfig = ref<SystemConfig | null>(null)
  const systemVersion = ref<string>('')

  // 各模块 loading 状态
  const siteInfoLoading = ref(false)
  const proxyConfigLoading = ref(false)
  const basicConfigLoading = ref(false)
  const logConfigLoading = ref(false)
  const contentModerationLoading = ref(false)
  const cleanupConfigLoading = ref(false)
  const providerAdvancedConfigLoading = ref(false)
  const providerAdvancedConfigReady = ref(false)
  const systemConfigLoading = ref(true)

  // 变动检测
  const hasSiteInfoChanges = computed(() => {
    if (!originalConfig.value) return false
    return (
      systemConfig.value.site_name !== originalConfig.value.site_name ||
      systemConfig.value.site_subtitle !== originalConfig.value.site_subtitle ||
      systemConfig.value.contact_us_format !== originalConfig.value.contact_us_format ||
      systemConfig.value.contact_us_content !== originalConfig.value.contact_us_content
    )
  })

  const hasProxyConfigChanges = computed(() => {
    if (!originalConfig.value) return false
    return systemConfig.value.system_proxy_node_id !== originalConfig.value.system_proxy_node_id
  })

  const hasBasicConfigChanges = computed(() => {
    if (!originalConfig.value) return false
    return (
      systemConfig.value.default_user_initial_gift_usd !== originalConfig.value.default_user_initial_gift_usd ||
      systemConfig.value.rate_limit_per_minute !== originalConfig.value.rate_limit_per_minute ||
      systemConfig.value.enable_registration !== originalConfig.value.enable_registration ||
      systemConfig.value.password_policy_level !== originalConfig.value.password_policy_level ||
      systemConfig.value.turnstile_enabled !== originalConfig.value.turnstile_enabled ||
      systemConfig.value.turnstile_site_key !== originalConfig.value.turnstile_site_key ||
      systemConfig.value.turnstile_secret_key.trim() !== '' ||
      JSON.stringify(systemConfig.value.turnstile_allowed_hostnames) !==
      JSON.stringify(originalConfig.value.turnstile_allowed_hostnames) ||
      systemConfig.value.referral_enabled !== originalConfig.value.referral_enabled ||
      systemConfig.value.referral_reward_mode !== originalConfig.value.referral_reward_mode ||
      systemConfig.value.referral_recharge_percent !== originalConfig.value.referral_recharge_percent ||
      systemConfig.value.referral_headcount_amount_usd !== originalConfig.value.referral_headcount_amount_usd ||
      systemConfig.value.referral_headcount_trigger !== originalConfig.value.referral_headcount_trigger ||
      systemConfig.value.registration_privacy_policy_enabled !==
      originalConfig.value.registration_privacy_policy_enabled ||
      systemConfig.value.registration_privacy_policy_format !==
      originalConfig.value.registration_privacy_policy_format ||
      systemConfig.value.registration_privacy_policy_content !==
      originalConfig.value.registration_privacy_policy_content ||
      systemConfig.value.registration_privacy_policy_version !==
      originalConfig.value.registration_privacy_policy_version ||
      systemConfig.value.auto_delete_expired_keys !== originalConfig.value.auto_delete_expired_keys ||
      systemConfig.value.enable_format_conversion !== originalConfig.value.enable_format_conversion ||
      systemConfig.value.enable_openai_image_sync_heartbeat !== originalConfig.value.enable_openai_image_sync_heartbeat
    )
  })

  const hasLogConfigChanges = computed(() => {
    if (!originalConfig.value) return false
    return (
      systemConfig.value.request_record_level !== originalConfig.value.request_record_level ||
      systemConfig.value.max_request_body_size !== originalConfig.value.max_request_body_size ||
      systemConfig.value.max_response_body_size !== originalConfig.value.max_response_body_size ||
      JSON.stringify(systemConfig.value.sensitive_headers) !==
      JSON.stringify(originalConfig.value.sensitive_headers)
    )
  })

  const hasContentModerationChanges = computed(() => {
    if (!originalConfig.value) return false
    return (
      JSON.stringify(systemConfig.value.content_moderation_account_protection) !==
      JSON.stringify(originalConfig.value.content_moderation_account_protection)
    )
  })

  const hasCleanupConfigChanges = computed(() => {
    if (!originalConfig.value) return false
    return (
      systemConfig.value.detail_log_retention_days !==
      originalConfig.value.detail_log_retention_days ||
      systemConfig.value.compressed_log_retention_days !==
      originalConfig.value.compressed_log_retention_days ||
      systemConfig.value.header_retention_days !== originalConfig.value.header_retention_days ||
      systemConfig.value.log_retention_days !== originalConfig.value.log_retention_days ||
      systemConfig.value.cleanup_batch_size !== originalConfig.value.cleanup_batch_size ||
      systemConfig.value.audit_log_retention_days !==
      originalConfig.value.audit_log_retention_days ||
      systemConfig.value.request_candidates_retention_days !==
      originalConfig.value.request_candidates_retention_days ||
      systemConfig.value.request_candidates_cleanup_batch_size !==
      originalConfig.value.request_candidates_cleanup_batch_size ||
      systemConfig.value.proxy_node_metrics_1m_retention_days !==
      originalConfig.value.proxy_node_metrics_1m_retention_days ||
      systemConfig.value.proxy_node_metrics_1h_retention_days !==
      originalConfig.value.proxy_node_metrics_1h_retention_days ||
      systemConfig.value.proxy_node_metrics_cleanup_batch_size !==
      originalConfig.value.proxy_node_metrics_cleanup_batch_size
    )
  })

  const hasProviderAdvancedConfigChanges = computed(() => {
    if (!originalConfig.value) return false
    return (
      systemConfig.value.codex_oauth_identity_convergence_enabled !==
      originalConfig.value.codex_oauth_identity_convergence_enabled
    )
  })

  // KB 和字节之间的转换
  const maxRequestBodySizeKB = computed({
    get: () => Math.round(systemConfig.value.max_request_body_size / 1024),
    set: (val: number) => {
      const nextValue = Number.isFinite(val) && val > 0 ? val : 256
      systemConfig.value.max_request_body_size = nextValue * 1024
    },
  })

  const maxResponseBodySizeKB = computed({
    get: () => Math.round(systemConfig.value.max_response_body_size / 1024),
    set: (val: number) => {
      const nextValue = Number.isFinite(val) && val > 0 ? val : 256
      systemConfig.value.max_response_body_size = nextValue * 1024
    },
  })

  // 敏感请求头数组和字符串之间的转换
  const sensitiveHeadersStr = computed({
    get: () => systemConfig.value.sensitive_headers.join(', '),
    set: (val: string) => {
      systemConfig.value.sensitive_headers = val
        .split(',')
        .map((s) => s.trim().toLowerCase())
        .filter((s) => s.length > 0)
    },
  })

  const turnstileAllowedHostnamesStr = computed({
    get: () => systemConfig.value.turnstile_allowed_hostnames.join(', '),
    set: (val: string) => {
      systemConfig.value.turnstile_allowed_hostnames = val
        .split(',')
        .map((s) => s.trim().toLowerCase())
        .filter((s) => s.length > 0)
    },
  })

  // 加载配置
  async function loadSystemConfig() {
    systemConfigLoading.value = true
    try {
      for (const key of CONFIG_KEYS) {
        try {
          const response = await adminApi.getSystemConfig(key)
          if (key === 'codex_oauth_identity_convergence_enabled') {
            if (typeof response.value !== 'boolean') {
              throw new Error('Codex OAuth 身份收敛配置必须是布尔值')
            }
            providerAdvancedConfigReady.value = true
          }
          if (key === 'turnstile_secret_key') {
            systemConfig.value.turnstile_secret_key = ''
            systemConfig.value.turnstile_secret_key_is_set = !!response.is_set
            continue
          }
          if (response.value !== null && response.value !== undefined) {
            ; (systemConfig.value as Record<string, unknown>)[key] =
              key === 'content_moderation_account_protection'
                ? normalizeContentModerationConfig(response.value)
                : response.value
          }
        } catch (err) {
          if (key === 'codex_oauth_identity_convergence_enabled') {
            providerAdvancedConfigReady.value = false
            error(t('systemConfigMessages.providerAdvancedLoadFailed'))
            log.error('加载 Provider 高级设置失败:', err)
          }
          // 单个配置项加载失败时忽略，使用默认值
        }
      }
      originalConfig.value = JSON.parse(JSON.stringify(systemConfig.value))
    } catch (err) {
      error(t('systemConfigMessages.loadFailed'))
      log.error('加载系统配置失败:', err)
    } finally {
      systemConfigLoading.value = false
    }
  }

  async function loadSystemVersion() {
    try {
      const data = await adminApi.getSystemVersion()
      systemVersion.value = data.version
    } catch (err) {
      log.error('加载系统版本失败:', err)
    }
  }

  // 保存函数
  async function saveSiteInfo() {
    siteInfoLoading.value = true
    try {
      const configItems = [
        { key: 'site_name', value: systemConfig.value.site_name, description: '站点名称' },
        {
          key: 'site_subtitle',
          value: systemConfig.value.site_subtitle,
          description: '站点副标题',
        },
        { key: 'contact_us_format', value: systemConfig.value.contact_us_format, description: '联系我们内容格式' },
        { key: 'contact_us_content', value: systemConfig.value.contact_us_content, description: '联系我们内容' },
      ]
      await Promise.all(
        configItems.map((item) =>
          adminApi.updateSystemConfig(item.key, item.value, item.description)
        )
      )
      if (originalConfig.value) {
        originalConfig.value.site_name = systemConfig.value.site_name
        originalConfig.value.site_subtitle = systemConfig.value.site_subtitle
        originalConfig.value.contact_us_format = systemConfig.value.contact_us_format
        originalConfig.value.contact_us_content = systemConfig.value.contact_us_content
      }
      await refreshSiteInfo()
      success(t('systemConfigMessages.siteSaved'))
    } catch (err) {
      error(t('systemConfigMessages.siteSaveFailed'))
      log.error('保存站点信息失败:', err)
    } finally {
      siteInfoLoading.value = false
    }
  }

  async function saveProxyConfig() {
    proxyConfigLoading.value = true
    try {
      await adminApi.updateSystemConfig(
        'system_proxy_node_id',
        systemConfig.value.system_proxy_node_id || null,
        '系统默认代理节点 ID'
      )
      if (originalConfig.value) {
        originalConfig.value.system_proxy_node_id = systemConfig.value.system_proxy_node_id
      }
      success(t('systemConfigMessages.proxySaved'))
    } catch (err) {
      error(t('systemConfigMessages.proxySaveFailed'))
      log.error('保存代理配置失败:', err)
    } finally {
      proxyConfigLoading.value = false
    }
  }

  async function saveBasicConfig() {
    basicConfigLoading.value = true
    try {
      const configItems = [
        {
          key: 'default_user_initial_gift_usd',
          value: systemConfig.value.default_user_initial_gift_usd,
          description: '默认用户初始赠款（美元）',
        },
        {
          key: 'rate_limit_per_minute',
          value: systemConfig.value.rate_limit_per_minute,
          description: '每分钟请求限制',
        },
        {
          key: 'enable_registration',
          value: systemConfig.value.enable_registration,
          description: '是否开放用户注册',
        },
        {
          key: 'password_policy_level',
          value: systemConfig.value.password_policy_level,
          description: '密码策略等级',
        },
        {
          key: 'turnstile_enabled',
          value: systemConfig.value.turnstile_enabled,
          description: 'Cloudflare Turnstile 注册人机验证开关',
        },
        {
          key: 'turnstile_site_key',
          value: systemConfig.value.turnstile_site_key?.trim() || null,
          description: 'Cloudflare Turnstile 站点 Key',
        },
        {
          key: 'turnstile_allowed_hostnames',
          value: systemConfig.value.turnstile_allowed_hostnames,
          description: 'Cloudflare Turnstile 允许的 hostname 列表',
        },
        {
          key: 'referral_enabled',
          value: systemConfig.value.referral_enabled,
          description: '邀请返利开关',
        },
        {
          key: 'referral_reward_mode',
          value: systemConfig.value.referral_reward_mode,
          description: '邀请返利方式',
        },
        {
          key: 'referral_recharge_percent',
          value: systemConfig.value.referral_recharge_percent,
          description: '邀请充值比例返利百分比',
        },
        {
          key: 'referral_headcount_amount_usd',
          value: systemConfig.value.referral_headcount_amount_usd,
          description: '邀请人头返利金额（美元）',
        },
        {
          key: 'referral_headcount_trigger',
          value: systemConfig.value.referral_headcount_trigger,
          description: '邀请人头返利触发时机',
        },
        {
          key: 'registration_privacy_policy_enabled',
          value: systemConfig.value.registration_privacy_policy_enabled,
          description: '注册隐私政策确认开关',
        },
        {
          key: 'registration_privacy_policy_format',
          value: systemConfig.value.registration_privacy_policy_format,
          description: '注册隐私政策内容格式',
        },
        {
          key: 'registration_privacy_policy_content',
          value: systemConfig.value.registration_privacy_policy_content,
          description: '注册隐私政策内容',
        },
        {
          key: 'registration_privacy_policy_version',
          value: systemConfig.value.registration_privacy_policy_version,
          description: '注册隐私政策版本',
        },
        {
          key: 'auto_delete_expired_keys',
          value: systemConfig.value.auto_delete_expired_keys,
          description: '是否自动删除过期的API Key',
        },
        {
          key: 'enable_format_conversion',
          value: systemConfig.value.enable_format_conversion,
          description: '全局格式转换开关：开启时强制允许所有提供商的格式转换',
        },
        {
          key: 'enable_openai_image_sync_heartbeat',
          value: systemConfig.value.enable_openai_image_sync_heartbeat,
          description: '同步生图保活：避免 CDN 超时，上游状态写入响应体',
        },
      ]
      const turnstileSecret = systemConfig.value.turnstile_secret_key.trim()
      if (turnstileSecret) {
        configItems.push({
          key: 'turnstile_secret_key',
          value: turnstileSecret,
          description: 'Cloudflare Turnstile Secret Key',
        })
      }

      await Promise.all(
        configItems.map((item) =>
          adminApi.updateSystemConfig(item.key, item.value, item.description)
        )
      )
      if (originalConfig.value) {
        originalConfig.value.default_user_initial_gift_usd = systemConfig.value.default_user_initial_gift_usd
        originalConfig.value.rate_limit_per_minute = systemConfig.value.rate_limit_per_minute
        originalConfig.value.enable_registration = systemConfig.value.enable_registration
        originalConfig.value.password_policy_level = systemConfig.value.password_policy_level
        originalConfig.value.turnstile_enabled = systemConfig.value.turnstile_enabled
        originalConfig.value.turnstile_site_key = systemConfig.value.turnstile_site_key?.trim() || null
        originalConfig.value.turnstile_allowed_hostnames = [
          ...systemConfig.value.turnstile_allowed_hostnames,
        ]
        originalConfig.value.referral_enabled = systemConfig.value.referral_enabled
        originalConfig.value.referral_reward_mode = systemConfig.value.referral_reward_mode
        originalConfig.value.referral_recharge_percent = systemConfig.value.referral_recharge_percent
        originalConfig.value.referral_headcount_amount_usd =
          systemConfig.value.referral_headcount_amount_usd
        originalConfig.value.referral_headcount_trigger =
          systemConfig.value.referral_headcount_trigger
        originalConfig.value.registration_privacy_policy_enabled =
          systemConfig.value.registration_privacy_policy_enabled
        originalConfig.value.registration_privacy_policy_format =
          systemConfig.value.registration_privacy_policy_format
        originalConfig.value.registration_privacy_policy_content =
          systemConfig.value.registration_privacy_policy_content
        originalConfig.value.registration_privacy_policy_version =
          systemConfig.value.registration_privacy_policy_version
        if (turnstileSecret) {
          systemConfig.value.turnstile_secret_key = ''
          systemConfig.value.turnstile_secret_key_is_set = true
          originalConfig.value.turnstile_secret_key = ''
          originalConfig.value.turnstile_secret_key_is_set = true
        }
        originalConfig.value.auto_delete_expired_keys =
          systemConfig.value.auto_delete_expired_keys
        originalConfig.value.enable_format_conversion =
          systemConfig.value.enable_format_conversion
        originalConfig.value.enable_openai_image_sync_heartbeat =
          systemConfig.value.enable_openai_image_sync_heartbeat
      }
      success(t('systemConfigMessages.basicSaved'))
    } catch (err) {
      error(t('systemConfigMessages.saveFailed'))
      log.error('保存基础配置失败:', err)
    } finally {
      basicConfigLoading.value = false
    }
  }

  async function clearTurnstileSecret() {
    basicConfigLoading.value = true
    try {
      await adminApi.updateSystemConfig(
        'turnstile_secret_key',
        '',
        'Cloudflare Turnstile Secret Key'
      )
      systemConfig.value.turnstile_secret_key = ''
      systemConfig.value.turnstile_secret_key_is_set = false
      if (originalConfig.value) {
        originalConfig.value.turnstile_secret_key = ''
        originalConfig.value.turnstile_secret_key_is_set = false
      }
      success(t('systemConfigMessages.turnstileCleared'))
    } catch (err) {
      error(t('systemConfigMessages.turnstileClearFailed'))
      log.error('清空 Turnstile 密钥失败:', err)
    } finally {
      basicConfigLoading.value = false
    }
  }

  async function saveLogConfig() {
    logConfigLoading.value = true
    try {
      const configItems = [
        {
          key: 'request_record_level',
          value: systemConfig.value.request_record_level,
          description: '请求记录级别',
        },
        {
          key: 'max_request_body_size',
          value: systemConfig.value.max_request_body_size,
          description: '最大请求体记录大小（字节）',
        },
        {
          key: 'max_response_body_size',
          value: systemConfig.value.max_response_body_size,
          description: '最大响应体记录大小（字节）',
        },
        {
          key: 'sensitive_headers',
          value: systemConfig.value.sensitive_headers,
          description: '敏感请求头列表',
        },
      ]

      await Promise.all(
        configItems.map((item) =>
          adminApi.updateSystemConfig(item.key, item.value, item.description)
        )
      )
      if (originalConfig.value) {
        originalConfig.value.request_record_level = systemConfig.value.request_record_level
        originalConfig.value.max_request_body_size = systemConfig.value.max_request_body_size
        originalConfig.value.max_response_body_size = systemConfig.value.max_response_body_size
        originalConfig.value.sensitive_headers = [...systemConfig.value.sensitive_headers]
      }
      success(t('systemConfigMessages.requestLogSaved'))
    } catch (err) {
      error(t('systemConfigMessages.saveFailed'))
      log.error('保存请求记录配置失败:', err)
    } finally {
      logConfigLoading.value = false
    }
  }

  async function saveContentModerationConfig() {
    contentModerationLoading.value = true
    try {
      const value = normalizeContentModerationConfig(
        systemConfig.value.content_moderation_account_protection,
      )
      const response = await adminApi.updateSystemConfig(
        'content_moderation_account_protection',
        value,
        '内容审查 / 账号保护配置',
      )
      const savedValue = normalizeContentModerationConfig(response.value ?? value)
      systemConfig.value.content_moderation_account_protection = savedValue
      if (originalConfig.value) {
        originalConfig.value.content_moderation_account_protection = JSON.parse(JSON.stringify(savedValue))
      }
      success(t('systemConfigMessages.moderationSaved'))
    } catch (err) {
      error(t('systemConfigMessages.moderationSaveFailed'))
      log.error('保存内容审查配置失败:', err)
    } finally {
      contentModerationLoading.value = false
    }
  }

  async function saveProviderAdvancedConfig() {
    providerAdvancedConfigLoading.value = true
    try {
      const value = systemConfig.value.codex_oauth_identity_convergence_enabled
      await adminApi.updateSystemConfig(
        'codex_oauth_identity_convergence_enabled',
        value,
        'Codex OAuth 身份收敛全局开关',
      )
      if (originalConfig.value) {
        originalConfig.value.codex_oauth_identity_convergence_enabled = value
      }
      success(t('systemConfigMessages.providerAdvancedSaved'))
    } catch (err) {
      error(t('systemConfigMessages.providerAdvancedSaveFailed'))
      log.error('保存 Provider 高级设置失败:', err)
    } finally {
      providerAdvancedConfigLoading.value = false
    }
  }

  async function saveCleanupConfig() {
    cleanupConfigLoading.value = true
    try {
      const configItems = [
        {
          key: 'detail_log_retention_days',
          value: systemConfig.value.detail_log_retention_days,
          description: '详细记录保留天数',
        },
        {
          key: 'compressed_log_retention_days',
          value: systemConfig.value.compressed_log_retention_days,
          description: '压缩记录保留天数',
        },
        {
          key: 'header_retention_days',
          value: systemConfig.value.header_retention_days,
          description: '请求头保留天数',
        },
        {
          key: 'log_retention_days',
          value: systemConfig.value.log_retention_days,
          description: '完整记录保留天数',
        },
        {
          key: 'cleanup_batch_size',
          value: systemConfig.value.cleanup_batch_size,
          description: '每批次清理的记录数',
        },
        {
          key: 'audit_log_retention_days',
          value: systemConfig.value.audit_log_retention_days,
          description: '审计日志保留天数',
        },
        {
          key: 'request_candidates_retention_days',
          value: systemConfig.value.request_candidates_retention_days,
          description: '请求候选记录保留天数',
        },
        {
          key: 'request_candidates_cleanup_batch_size',
          value: systemConfig.value.request_candidates_cleanup_batch_size,
          description: '请求候选记录每批次清理条数',
        },
        {
          key: 'proxy_node_metrics_1m_retention_days',
          value: systemConfig.value.proxy_node_metrics_1m_retention_days,
          description: '代理节点 1m 指标保留天数',
        },
        {
          key: 'proxy_node_metrics_1h_retention_days',
          value: systemConfig.value.proxy_node_metrics_1h_retention_days,
          description: '代理节点 1h 指标保留天数',
        },
        {
          key: 'proxy_node_metrics_cleanup_batch_size',
          value: systemConfig.value.proxy_node_metrics_cleanup_batch_size,
          description: '代理节点指标每批次清理条数',
        },
      ]

      await Promise.all(
        configItems.map((item) =>
          adminApi.updateSystemConfig(item.key, item.value, item.description)
        )
      )
      if (originalConfig.value) {
        originalConfig.value.detail_log_retention_days =
          systemConfig.value.detail_log_retention_days
        originalConfig.value.compressed_log_retention_days =
          systemConfig.value.compressed_log_retention_days
        originalConfig.value.header_retention_days = systemConfig.value.header_retention_days
        originalConfig.value.log_retention_days = systemConfig.value.log_retention_days
        originalConfig.value.cleanup_batch_size = systemConfig.value.cleanup_batch_size
        originalConfig.value.audit_log_retention_days =
          systemConfig.value.audit_log_retention_days
        originalConfig.value.request_candidates_retention_days =
          systemConfig.value.request_candidates_retention_days
        originalConfig.value.request_candidates_cleanup_batch_size =
          systemConfig.value.request_candidates_cleanup_batch_size
        originalConfig.value.proxy_node_metrics_1m_retention_days =
          systemConfig.value.proxy_node_metrics_1m_retention_days
        originalConfig.value.proxy_node_metrics_1h_retention_days =
          systemConfig.value.proxy_node_metrics_1h_retention_days
        originalConfig.value.proxy_node_metrics_cleanup_batch_size =
          systemConfig.value.proxy_node_metrics_cleanup_batch_size
      }
      success(t('systemConfigMessages.cleanupSaved'))
    } catch (err) {
      error(t('systemConfigMessages.saveFailed'))
      log.error('保存请求记录清理配置失败:', err)
    } finally {
      cleanupConfigLoading.value = false
    }
  }

  async function handleAutoCleanupToggle(enabled: boolean) {
    const previousValue = systemConfig.value.enable_auto_cleanup
    systemConfig.value.enable_auto_cleanup = enabled
    try {
      await adminApi.updateSystemConfig(
        'enable_auto_cleanup',
        enabled,
        '是否启用自动清理任务'
      )
      success(enabled ? t('systemConfigMessages.autoCleanupEnabled') : t('systemConfigMessages.autoCleanupDisabled'))
    } catch (err) {
      error(t('systemConfigMessages.saveFailed'))
      log.error('保存自动清理配置失败:', err)
      systemConfig.value.enable_auto_cleanup = previousValue
    }
  }

  return {
    systemConfig,
    originalConfig,
    systemVersion,
    // loading 状态
    siteInfoLoading,
    proxyConfigLoading,
    basicConfigLoading,
    logConfigLoading,
    contentModerationLoading,
    cleanupConfigLoading,
    providerAdvancedConfigLoading,
    providerAdvancedConfigReady,
    systemConfigLoading,
    // 变动检测
    hasSiteInfoChanges,
    hasProxyConfigChanges,
    hasBasicConfigChanges,
    hasLogConfigChanges,
    hasContentModerationChanges,
    hasCleanupConfigChanges,
    hasProviderAdvancedConfigChanges,
    // 计算属性
    maxRequestBodySizeKB,
    maxResponseBodySizeKB,
    sensitiveHeadersStr,
    turnstileAllowedHostnamesStr,
    // 加载函数
    loadSystemConfig,
    loadSystemVersion,
    // 保存函数
    saveSiteInfo,
    saveProxyConfig,
    saveBasicConfig,
    clearTurnstileSecret,
    saveLogConfig,
    saveContentModerationConfig,
    saveProviderAdvancedConfig,
    saveCleanupConfig,
    handleAutoCleanupToggle,
  }
}
