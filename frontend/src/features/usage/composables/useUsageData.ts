import { ref, computed, type Ref } from 'vue'
import { usageApi } from '@/api/usage'
import { meApi } from '@/api/me'
import type {
  UsageStatsState,
  ModelStatsItem,
  ProviderStatsItem,
  ApiFormatStatsItem,
  UsageRecord,
  DateRangeParams,
  EnhancedModelStatsItem
} from '../types'
import { createDefaultStats } from '../types'
import { log } from '@/utils/logger'
import { getErrorStatus } from '@/types/api-error'

export interface UseUsageDataOptions {
  isAdminPage: Ref<boolean>
}

export interface PaginationParams {
  page: number
  pageSize: number
}

export interface FilterParams {
  search?: string
  user_id?: string
  api_key_group_id?: string
  model?: string
  provider?: string
  api_format?: string
  status?: string
  client_family?: string
}

function isUsageProviderVisible(provider: string | undefined | null): provider is string {
  const normalized = provider?.trim().toLowerCase()
  return !!normalized && !['unknown', 'unknow', 'pending'].includes(normalized)
}

export function useUsageData(options: UseUsageDataOptions) {
  const { isAdminPage } = options

  // 加载状态
  const isLoadingStats = ref(true)
  const isLoadingRecords = ref(false)
  const loading = computed(() => isLoadingStats.value || isLoadingRecords.value)

  // 统计数据
  const stats = ref<UsageStatsState>(createDefaultStats())
  const modelStats = ref<ModelStatsItem[]>([])
  const providerStats = ref<ProviderStatsItem[]>([])
  const apiFormatStats = ref<ApiFormatStatsItem[]>([])

  // 记录数据 - 只存储当前页
  const currentRecords = ref<UsageRecord[]>([])
  const totalRecords = ref(0)

  // 当前的日期范围（用于分页请求）
  const currentDateRange = ref<DateRangeParams | undefined>(undefined)
  let loadStatsRequestId = 0
  let loadRecordsRequestId = 0

  // 可用的筛选选项（从统计数据获取，而不是从记录中）
  const availableModels = ref<string[]>([])
  const availableProviders = ref<string[]>([])

  // 增强的模型统计（包含效率分析）
  const enhancedModelStats = computed<EnhancedModelStatsItem[]>(() => {
    return modelStats.value.map(model => ({
      ...model,
      costPerToken: model.total_tokens > 0
        ? `$${(model.total_cost / model.total_tokens * 1000000).toFixed(2)}/M`
        : '-'
    }))
  })

  // 加载统计数据（不加载记录）
  async function loadStats(dateRange?: DateRangeParams): Promise<boolean> {
    const requestId = ++loadStatsRequestId
    isLoadingStats.value = true
    currentDateRange.value = dateRange

    try {
      if (isAdminPage.value) {
        // 管理员页面顺序加载统计数据，避免刷新使用记录时瞬时打满后端 worker。
        stats.value = createDefaultStats()
        modelStats.value = []
        providerStats.value = []
        apiFormatStats.value = []
        availableModels.value = []
        availableProviders.value = []

        let hadFailure = false
        const markFailure = (error: unknown) => {
          hadFailure = true
          if (getErrorStatus(error) !== 403) {
            log.error('加载统计数据失败:', error)
          }
        }

        try {
          const statsData = await usageApi.getUsageStats(dateRange)
          if (requestId !== loadStatsRequestId) {
            return true
          }

          // statsData may contain additional fields not declared in UsageStats
          const statsRaw = statsData as unknown as Record<string, unknown>
          stats.value = {
            total_requests: statsData.total_requests || 0,
            total_tokens: statsData.total_tokens || 0,
            total_cost: statsData.total_cost || 0,
            total_actual_cost: statsData.total_actual_cost,
            avg_response_time: statsData.avg_response_time || 0,
            error_count: typeof statsRaw.error_count === 'number' ? statsRaw.error_count : undefined,
            error_rate: typeof statsRaw.error_rate === 'number' ? statsRaw.error_rate : undefined,
            cache_stats: statsRaw.cache_stats as UsageStatsState['cache_stats'],
            period_start: '',
            period_end: '',
          }
        } catch (error) {
          if (requestId !== loadStatsRequestId) {
            return true
          }
          markFailure(error)
        }

        try {
          const modelData = await usageApi.getUsageByModel(dateRange)
          if (requestId !== loadStatsRequestId) {
            return true
          }

          modelStats.value = modelData.map(item => {
            const raw = item as unknown as Record<string, unknown>
            return {
              model: item.model,
              request_count: item.request_count || 0,
              total_tokens: item.total_tokens || 0,
              effective_input_tokens: typeof raw.effective_input_tokens === 'number' ? raw.effective_input_tokens : 0,
              total_input_context: typeof raw.total_input_context === 'number' ? raw.total_input_context : 0,
              output_tokens: typeof raw.output_tokens === 'number' ? raw.output_tokens : 0,
              cache_read_tokens: typeof raw.cache_read_tokens === 'number' ? raw.cache_read_tokens : 0,
              cache_creation_tokens: typeof raw.cache_creation_tokens === 'number' ? raw.cache_creation_tokens : 0,
              cache_hit_rate: typeof raw.cache_hit_rate === 'number' ? raw.cache_hit_rate : 0,
              official_cost: typeof raw.official_cost === 'number' ? raw.official_cost : undefined,
              total_cost: item.total_cost || 0,
              actual_cost: typeof raw.actual_cost === 'number' ? raw.actual_cost : undefined
            }
          })

          availableModels.value = modelData.map(item => item.model).filter(Boolean).sort()
        } catch (error) {
          if (requestId !== loadStatsRequestId) {
            return true
          }
          markFailure(error)
        }

        try {
          const providerData = await usageApi.getUsageByProvider(dateRange)
          if (requestId !== loadStatsRequestId) {
            return true
          }

          const visibleProviderData = providerData.filter(item => isUsageProviderVisible(item.provider))
          providerStats.value = visibleProviderData.map(item => ({
            provider: item.provider,
            requests: item.request_count,
            totalTokens: item.total_tokens || 0,
            effectiveInputTokens: item.effective_input_tokens || 0,
            totalInputContext: item.total_input_context || 0,
            outputTokens: item.output_tokens || 0,
            cacheReadTokens: item.cache_read_tokens || 0,
            cacheCreationTokens: item.cache_creation_tokens || 0,
            cacheHitRate: item.cache_hit_rate || 0,
            totalCost: item.total_cost,
            actualCost: item.actual_cost,
            successRate: item.success_rate,
            avgResponseTime: item.avg_response_time_ms > 0
              ? `${(item.avg_response_time_ms / 1000).toFixed(2)}s`
              : '-'
          }))

          availableProviders.value = visibleProviderData.map(item => item.provider).sort()
        } catch (error) {
          if (requestId !== loadStatsRequestId) {
            return true
          }
          markFailure(error)
        }

        try {
          const apiFormatData = await usageApi.getUsageByApiFormat(dateRange)
          if (requestId !== loadStatsRequestId) {
            return true
          }

          apiFormatStats.value = apiFormatData.map(item => ({
            api_format: item.api_format,
            request_count: item.request_count || 0,
            total_tokens: item.total_tokens || 0,
            effective_input_tokens: item.effective_input_tokens || 0,
            total_input_context: item.total_input_context || 0,
            output_tokens: item.output_tokens || 0,
            cache_read_tokens: item.cache_read_tokens || 0,
            cache_creation_tokens: item.cache_creation_tokens || 0,
            cache_hit_rate: item.cache_hit_rate || 0,
            total_cost: item.total_cost || 0,
            actual_cost: item.actual_cost,
            avgResponseTime: item.avg_response_time_ms > 0
              ? `${(item.avg_response_time_ms / 1000).toFixed(2)}s`
              : '-'
          }))
        } catch (error) {
          if (requestId !== loadStatsRequestId) {
            return true
          }
          markFailure(error)
        }

        return hadFailure
      }

      // 用户页面
      const userData = await meApi.getUsage({
        ...dateRange,
        include_records: false
      })
      if (requestId !== loadStatsRequestId) {
        return false
      }

      stats.value = {
        total_requests: userData.total_requests || 0,
        total_tokens: userData.total_tokens || 0,
        total_cost: userData.total_cost || 0,
        total_actual_cost: userData.total_actual_cost,
        avg_response_time: userData.avg_response_time || 0,
        period_start: '',
        period_end: '',
      }

      modelStats.value = (userData.summary_by_model || []).map((item) => ({
        model: item.model,
        request_count: item.requests || 0,
        total_tokens: item.total_tokens || 0,
        effective_input_tokens: item.effective_input_tokens || 0,
        total_input_context: item.total_input_context || 0,
        output_tokens: item.output_tokens || 0,
        cache_read_tokens: item.cache_read_tokens || 0,
        cache_creation_tokens: item.cache_creation_tokens || 0,
        cache_hit_rate: item.cache_hit_rate || 0,
        official_cost: typeof item.official_cost === 'number' ? item.official_cost : undefined,
        total_cost: item.total_cost_usd || 0,
        actual_cost: item.actual_total_cost_usd
      }))

      providerStats.value = (userData.summary_by_provider || [])
        .filter((item) => isUsageProviderVisible(item.provider))
        .map((item) => ({
          provider: item.provider,
          requests: item.requests || 0,
          totalTokens: item.total_tokens || 0,
          effectiveInputTokens: item.effective_input_tokens || 0,
          totalInputContext: item.total_input_context || 0,
          outputTokens: item.output_tokens || 0,
          cacheReadTokens: item.cache_read_tokens || 0,
          cacheCreationTokens: item.cache_creation_tokens || 0,
          cacheHitRate: item.cache_hit_rate || 0,
          totalCost: item.total_cost_usd || 0,
          successRate: item.success_rate || 0,
          avgResponseTime: (item.avg_response_time_ms ?? 0) > 0
            ? `${((item.avg_response_time_ms ?? 0) / 1000).toFixed(2)}s`
            : '-'
        }))

      availableModels.value = modelStats.value.map(item => item.model).filter(Boolean).sort()
      availableProviders.value = (userData.summary_by_provider || [])
        .map(item => item.provider)
        .filter(isUsageProviderVisible)
        .sort()

      // API 格式统计直接使用后端聚合数据
      apiFormatStats.value = (userData.summary_by_api_format || []).map(item => ({
        api_format: item.api_format,
        request_count: item.request_count || 0,
        total_tokens: item.total_tokens || 0,
        effective_input_tokens: item.effective_input_tokens || 0,
        total_input_context: item.total_input_context || 0,
        output_tokens: item.output_tokens || 0,
        cache_read_tokens: item.cache_read_tokens || 0,
        cache_creation_tokens: item.cache_creation_tokens || 0,
        cache_hit_rate: item.cache_hit_rate || 0,
        total_cost: item.total_cost_usd || 0,
        avgResponseTime: (item.avg_response_time_ms ?? 0) > 0
          ? `${((item.avg_response_time_ms ?? 0) / 1000).toFixed(2)}s`
          : '-'
      }))

      return false
    } catch (error: unknown) {
      if (requestId !== loadStatsRequestId) {
        return true
      }
      if (getErrorStatus(error) !== 403) {
        log.error('加载统计数据失败:', error)
      }
      stats.value = createDefaultStats()
      modelStats.value = []
      providerStats.value = []
      apiFormatStats.value = []
      availableModels.value = []
      availableProviders.value = []
      return true
    } finally {
      if (requestId === loadStatsRequestId) {
        isLoadingStats.value = false
      }
    }
  }

  // 加载记录（真正的后端分页）
  async function loadRecords(
    pagination: PaginationParams,
    filters?: FilterParams,
    dateRange?: DateRangeParams
  ): Promise<void> {
    const requestId = ++loadRecordsRequestId
    isLoadingRecords.value = true

    try {
      const offset = (pagination.page - 1) * pagination.pageSize
      const effectiveDateRange = dateRange ?? currentDateRange.value
      if (dateRange) {
        currentDateRange.value = dateRange
      }

      // 构建请求参数
      const params: Record<string, unknown> = {
        limit: pagination.pageSize,
        offset,
        ...effectiveDateRange
      }

      // 添加筛选条件
      if (filters?.search?.trim()) {
        params.search = filters.search.trim()
      }

      if (isAdminPage.value) {
        // 管理员页面：使用管理员 API
        if (filters?.user_id) {
          params.user_id = filters.user_id
        }
        if (filters?.api_key_group_id) {
          params.api_key_group_id = filters.api_key_group_id
        }
        if (filters?.model) {
          params.model = filters.model
        }
        if (filters?.provider) {
          params.provider = filters.provider
        }
        if (filters?.api_format) {
          params.api_format = filters.api_format
        }
        if (filters?.status) {
          params.status = filters.status
        }
        if (filters?.client_family) {
          params.client_family = filters.client_family
        }

        const response = await usageApi.getAllUsageRecords(params)
        if (requestId !== loadRecordsRequestId) {
          return
        }
        const nextRecords = (response.records || []) as unknown as UsageRecord[]
        currentRecords.value = mergeRecordStatus(currentRecords.value, nextRecords)
        totalRecords.value = response.total || 0
      } else {
        // 用户页面：使用用户 API
        if (filters?.model) {
          params.model = filters.model
        }
        if (filters?.provider) {
          params.provider = filters.provider
        }
        if (filters?.api_format) {
          params.api_format = filters.api_format
        }
        if (filters?.status) {
          params.status = filters.status
        }
        if (filters?.client_family) {
          params.client_family = filters.client_family
        }

        const userData = await meApi.getUsage(params)
        if (requestId !== loadRecordsRequestId) {
          return
        }
        const nextRecords = (userData.records || []) as UsageRecord[]
        currentRecords.value = mergeRecordStatus(currentRecords.value, nextRecords)
        totalRecords.value = userData.pagination?.total ?? currentRecords.value.length
      }
    } catch (error) {
      if (requestId !== loadRecordsRequestId) {
        return
      }
      log.error('加载记录失败:', error)
      currentRecords.value = []
      totalRecords.value = 0
    } finally {
      if (requestId === loadRecordsRequestId) {
        isLoadingRecords.value = false
      }
    }
  }

  function mergeRecordStatus(
    current: UsageRecord[],
    next: UsageRecord[]
  ): UsageRecord[] {
    if (!current.length) return next
    const statusPriority: Record<string, number> = {
      pending: 0,
      streaming: 1,
      completed: 2,
      failed: 2,
      cancelled: 2
    }
    const currentById = new Map<string, UsageRecord>(
      current.map(record => [record.id, record])
    )
    return next.map(record => {
      const existing = currentById.get(record.id)
      if (!existing) return record

      // 确定是否需要保护 status（避免刷新把已知状态覆盖为 undefined 或回退）
      const hasExistingStatus = typeof existing.status === 'string' && existing.status.length > 0
      const hasNextStatus = typeof record.status === 'string' && record.status.length > 0
      const existingStatus = hasExistingStatus ? existing.status as UsageRecord['status'] : undefined
      const nextStatus = hasNextStatus ? record.status as UsageRecord['status'] : undefined
      const currentRank = existingStatus ? (statusPriority[existingStatus] ?? -1) : -1
      const nextRank = nextStatus ? (statusPriority[nextStatus] ?? -1) : -1
      const statusProgressed = hasNextStatus && (
        !hasExistingStatus ||
        nextRank > currentRank ||
        (nextRank === currentRank && existing.status === record.status)
      )
      const mergedStatus = statusProgressed ? record.status : existing.status
      const protectStatus = mergedStatus !== record.status

      // 确定是否需要保护 provider（避免 pending/unknown/unknow 覆盖已有的正确值）
      const isPendingProvider = !isUsageProviderVisible(record.provider)
      const hasValidExistingProvider = isUsageProviderVisible(existing.provider)
      const protectProvider = isPendingProvider && hasValidExistingProvider

      // 如果需要保护状态，说明本地数据比后端更新，应该保留本地的所有实时更新字段
      if (protectStatus) {
        const recordUpstreamIsStream = typeof record.upstream_is_stream === 'boolean'
          ? record.upstream_is_stream
          : typeof record.is_stream === 'boolean'
            ? record.is_stream
            : undefined
        const existingUpstreamIsStream = typeof existing.upstream_is_stream === 'boolean'
          ? existing.upstream_is_stream
          : typeof existing.is_stream === 'boolean'
            ? existing.is_stream
            : undefined
        const upstreamIsStream = recordUpstreamIsStream ?? existingUpstreamIsStream ?? false

        const recordClientRequestedStream = typeof record.client_requested_stream === 'boolean'
          ? record.client_requested_stream
          : typeof record.client_is_stream === 'boolean'
            ? record.client_is_stream
            : undefined
        const existingClientRequestedStream = typeof existing.client_requested_stream === 'boolean'
          ? existing.client_requested_stream
          : typeof existing.client_is_stream === 'boolean'
            ? existing.client_is_stream
            : undefined
        const clientRequestedStream = recordClientRequestedStream ?? existingClientRequestedStream

        const recordClientIsStream = typeof record.client_is_stream === 'boolean'
          ? record.client_is_stream
          : typeof record.client_requested_stream === 'boolean'
            ? record.client_requested_stream
            : undefined
        const existingClientIsStream = typeof existing.client_is_stream === 'boolean'
          ? existing.client_is_stream
          : typeof existing.client_requested_stream === 'boolean'
            ? existing.client_requested_stream
            : undefined
        const clientIsStream = recordClientIsStream ?? existingClientIsStream ?? clientRequestedStream

        return {
          ...record,
          // 保留本地的状态和所有通过轮询更新的字段
          status: mergedStatus,
          provider: protectProvider ? existing.provider : (record.provider || existing.provider),
          input_tokens: Number.isFinite(record.input_tokens)
            ? record.input_tokens
            : existing.input_tokens,
          effective_input_tokens: record.effective_input_tokens ?? existing.effective_input_tokens,
          output_tokens: existing.output_tokens || record.output_tokens,
          cache_creation_input_tokens: existing.cache_creation_input_tokens ?? record.cache_creation_input_tokens,
          cache_creation_ephemeral_5m_input_tokens:
            existing.cache_creation_ephemeral_5m_input_tokens
            ?? record.cache_creation_ephemeral_5m_input_tokens,
          cache_creation_ephemeral_1h_input_tokens:
            existing.cache_creation_ephemeral_1h_input_tokens
            ?? record.cache_creation_ephemeral_1h_input_tokens,
          cache_read_input_tokens: existing.cache_read_input_tokens ?? record.cache_read_input_tokens,
          official_cost: record.official_cost ?? existing.official_cost,
          cost: Number.isFinite(record.cost) ? record.cost : existing.cost,
          sales_multiplier: record.sales_multiplier ?? existing.sales_multiplier,
          discount: record.discount ?? existing.discount,
          actual_cost: record.actual_cost ?? existing.actual_cost,
          charge_breakdown: record.charge_breakdown ?? existing.charge_breakdown,
          response_time_ms: existing.response_time_ms ?? record.response_time_ms,
          first_byte_time_ms: existing.first_byte_time_ms ?? record.first_byte_time_ms,
          is_stream: upstreamIsStream,
          upstream_is_stream: upstreamIsStream,
          client_requested_stream: clientRequestedStream,
          client_is_stream: clientIsStream,
          api_format: existing.api_format || record.api_format,
          endpoint_api_format: existing.endpoint_api_format || record.endpoint_api_format,
          has_format_conversion: existing.has_format_conversion ?? record.has_format_conversion,
          has_fallback: typeof record.has_fallback === 'boolean'
            ? record.has_fallback
            : existing.has_fallback,
          provider_route: Array.isArray(record.provider_route)
            ? record.provider_route
            : existing.provider_route,
          api_key_name: existing.api_key_name || record.api_key_name,
          provider_key_name: existing.provider_key_name || record.provider_key_name,
          provider_key_account_label: existing.provider_key_account_label || record.provider_key_account_label,
          rate_multiplier: record.rate_multiplier ?? existing.rate_multiplier,
          target_model: existing.target_model || record.target_model,
          requested_reasoning_effort: record.requested_reasoning_effort
            ?? existing.requested_reasoning_effort,
          reasoning_effort: record.reasoning_effort ?? existing.reasoning_effort,
        }
      }

      // 只需要保护 provider
      if (protectProvider) {
        return {
          ...record,
          provider: existing.provider
        }
      }

      return record
    })
  }

  // 刷新所有数据
  async function refreshData(dateRange?: DateRangeParams) {
    await loadStats(dateRange)
  }

  return {
    // 状态
    loading,
    isLoadingStats,
    isLoadingRecords,
    stats,
    modelStats,
    providerStats,
    apiFormatStats,
    currentRecords,
    totalRecords,

    // 筛选选项
    availableModels,
    availableProviders,

    // 计算属性
    enhancedModelStats,

    // 方法
    loadStats,
    loadRecords,
    refreshData
  }
}
