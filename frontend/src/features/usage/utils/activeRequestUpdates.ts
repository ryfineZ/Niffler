import type { UsageRecord } from '../types'

type NullableUsageRecordUpdate = {
  [K in keyof Omit<UsageRecord, 'id'>]?: UsageRecord[K] | null
}

export type UsageActiveRequestUpdate = NullableUsageRecordUpdate & {
  id: string
}

function isVisibleProvider(provider: string | undefined | null): provider is string {
  const normalized = provider?.trim().toLowerCase()
  return !!normalized && !['pending', 'unknown', 'unknow'].includes(normalized)
}

export function applyUsageActiveRequestUpdate(
  record: UsageRecord,
  update: UsageActiveRequestUpdate,
): void {
  record.status = update.status ?? record.status
  if (typeof update.input_tokens === 'number') record.input_tokens = update.input_tokens
  record.effective_input_tokens = update.effective_input_tokens ?? record.effective_input_tokens
  if (typeof update.output_tokens === 'number') record.output_tokens = update.output_tokens
  record.cache_creation_input_tokens = update.cache_creation_input_tokens ?? undefined
  record.cache_creation_ephemeral_5m_input_tokens =
    update.cache_creation_ephemeral_5m_input_tokens ?? undefined
  record.cache_creation_ephemeral_1h_input_tokens =
    update.cache_creation_ephemeral_1h_input_tokens ?? undefined
  record.cache_read_input_tokens = update.cache_read_input_tokens ?? undefined
  record.official_cost = update.official_cost ?? undefined
  if (typeof update.cost === 'number') record.cost = update.cost
  record.sales_multiplier = update.sales_multiplier ?? undefined
  record.discount = update.discount ?? undefined
  record.actual_cost = update.actual_cost ?? undefined
  record.rate_multiplier = update.rate_multiplier ?? undefined
  record.response_time_ms = update.response_time_ms ?? undefined
  record.first_byte_time_ms = update.first_byte_time_ms ?? undefined
  record.status_code = update.status_code ?? undefined
  record.error_message = update.error_message ?? undefined

  if ('charge_breakdown' in update) {
    record.charge_breakdown = update.charge_breakdown ?? null
  }

  if (typeof update.upstream_is_stream === 'boolean') {
    record.upstream_is_stream = update.upstream_is_stream
    record.is_stream = update.upstream_is_stream
  } else if (typeof update.is_stream === 'boolean') {
    record.is_stream = update.is_stream
    record.upstream_is_stream = update.is_stream
  }

  if (typeof update.client_is_stream === 'boolean') {
    record.client_is_stream = update.client_is_stream
    record.client_requested_stream = update.client_is_stream
  } else if (typeof update.client_requested_stream === 'boolean') {
    record.client_requested_stream = update.client_requested_stream
    record.client_is_stream = update.client_requested_stream
  }

  if (update.api_format != null) record.api_format = update.api_format
  if (update.endpoint_api_format != null) record.endpoint_api_format = update.endpoint_api_format
  if (update.has_format_conversion != null) {
    record.has_format_conversion = update.has_format_conversion
  }
  if (typeof update.has_fallback === 'boolean') {
    record.has_fallback = update.has_fallback
  }

  if ('target_model' in update && (typeof update.target_model === 'string' || update.target_model === null)) {
    record.target_model = update.target_model
  }
  if ('requested_reasoning_effort' in update) {
    record.requested_reasoning_effort = typeof update.requested_reasoning_effort === 'string'
      ? update.requested_reasoning_effort
      : null
  }
  if ('reasoning_effort' in update) {
    record.reasoning_effort = typeof update.reasoning_effort === 'string'
      ? update.reasoning_effort
      : null
  }

  if ('provider' in update && typeof update.provider === 'string' && isVisibleProvider(update.provider)) {
    record.provider = update.provider
  }
  if (Array.isArray(update.provider_route)) {
    record.provider_route = update.provider_route.filter((item): item is string => (
      typeof item === 'string' && item.trim().length > 0
    ))
  }
  if ('api_key_name' in update) {
    record.api_key_name = typeof update.api_key_name === 'string' ? update.api_key_name : undefined
  }
  if ('provider_key_name' in update) {
    record.provider_key_name = typeof update.provider_key_name === 'string'
      ? update.provider_key_name
      : undefined
  }
  if ('provider_key_account_label' in update) {
    record.provider_key_account_label = typeof update.provider_key_account_label === 'string'
      ? update.provider_key_account_label
      : undefined
  }
}
