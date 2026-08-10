import apiClient from './client'
import type { PaymentOrder } from './wallet'
import type { BillingEntitlementsInput } from '@/utils/billingEntitlements'

export type BillingDurationUnit = 'day' | 'month' | 'year' | 'custom'
export type BillingPurchaseLimitScope = 'active_period' | 'lifetime' | 'unlimited'
export type WalletCreditBucket = 'recharge' | 'gift'

export interface EpayChannelConfig {
  channel: string
  display_name: string
}

export type PaymentGatewayProvider = 'epay' | 'dodopay'

export interface PaymentGatewayConfig {
  provider: PaymentGatewayProvider
  enabled: boolean
  endpoint_url?: string | null
  callback_base_url?: string | null
  merchant_id?: string | null
  has_secret: boolean
  pay_currency?: string | null
  usd_exchange_rate?: number | null
  min_recharge_usd?: number | null
  channels?: EpayChannelConfig[]
  created_at?: number | null
  updated_at?: number | null
}

export type EpayGatewayConfig = PaymentGatewayConfig & { provider: 'epay' }
export type DodopayGatewayConfig = PaymentGatewayConfig & { provider: 'dodopay' }

export interface UpdatePaymentGatewayConfigRequest {
  enabled: boolean
  endpoint_url: string
  callback_base_url?: string | null
  merchant_id: string
  merchant_key?: string
  pay_currency: string
  usd_exchange_rate: number
  min_recharge_usd: number
  channels: EpayChannelConfig[]
}

export type UpdateEpayGatewayConfigRequest = UpdatePaymentGatewayConfigRequest
export type UpdateDodopayGatewayConfigRequest = UpdatePaymentGatewayConfigRequest

export interface GatewayTestResponse {
  ok: boolean
  provider: string
}

export interface WalletCreditEntitlement {
  type: 'wallet_credit'
  amount_usd: number
  balance_bucket?: WalletCreditBucket
}

export interface DailyQuotaEntitlement {
  type: 'daily_quota'
  allowed_global_model_ids?: string[]
  daily_quota_usd?: number
  five_hour_quota_usd?: number
  weekly_quota_usd?: number
  monthly_quota_usd?: number
  quota_multiplier?: number
  rpm_limit?: number
  reset_timezone?: string
  carry_over?: boolean
  allow_wallet_overage?: boolean
  limits?: {
    daily_limit_usd?: string | number
    five_hour_limit_usd?: string | number
    weekly_limit_usd?: string | number
    monthly_limit_usd?: string | number
    rpm_limit?: string | number
  }
}

export interface MembershipGroupEntitlement {
  type: 'membership_group'
  grant_user_groups: string[]
}

export type BillingEntitlement =
  | WalletCreditEntitlement
  | DailyQuotaEntitlement
  | MembershipGroupEntitlement

export interface BillingPlan {
  id: string
  title: string
  description?: string | null
  price_amount: number
  price_currency: string
  duration_unit: BillingDurationUnit
  duration_value: number
  enabled: boolean
  sort_order: number
  max_active_per_user: number
  purchase_limit_scope: BillingPurchaseLimitScope
  allowed_provider_ids: string[]
  entitlements: BillingEntitlementsInput
  created_at?: number | null
  updated_at?: number | null
}

export interface BillingPlanWriteRequest {
  title: string
  description?: string | null
  price_amount: number
  price_currency: string
  duration_unit: BillingDurationUnit
  duration_value: number
  enabled: boolean
  sort_order: number
  max_active_per_user: number
  purchase_limit_scope: BillingPurchaseLimitScope
  allowed_provider_ids: string[]
  entitlements: BillingEntitlementsInput
}

export interface BillingPlanListResponse {
  items: BillingPlan[]
  total: number
}

export interface BillingCheckoutRequest {
  payment_method?: string
  payment_provider?: string
  payment_channel?: string
}

export interface BillingCheckoutResponse {
  order: PaymentOrder & {
    order_kind?: string
    product_id?: string | null
    product?: BillingPlan | null
    plan_amount_usd?: number
    debt_repayment_usd: number
  }
  payment_instructions: Record<string, unknown>
}

export interface UserPlanEntitlement {
  id: string
  user_id: string
  plan_id: string
  payment_order_id: string
  status: string
  starts_at: string | null
  expires_at: string | null
  allowed_provider_ids: string[]
  entitlements: BillingEntitlementsInput
  active?: boolean
  created_at?: string | null
  updated_at?: string | null
}

export interface UserPlanEntitlementsResponse {
  items: UserPlanEntitlement[]
  total: number
}

function normalizeChannels(channels: PaymentGatewayConfig['channels']): EpayChannelConfig[] {
  return Array.isArray(channels)
    ? channels
      .map((item) => {
        const raw = item as EpayChannelConfig & { type?: string }
        const channel = String(raw.channel || raw.type || '').trim()
        return {
          channel,
          display_name: String(raw.display_name || channel).trim(),
        }
      })
      .filter((item) => item.channel && item.display_name)
    : []
}

function normalizeGatewayConfig(
  config: PaymentGatewayConfig,
  provider: PaymentGatewayProvider
): PaymentGatewayConfig {
  return {
    provider,
    enabled: Boolean(config.enabled),
    endpoint_url: config.endpoint_url ?? '',
    callback_base_url: config.callback_base_url ?? '',
    merchant_id: config.merchant_id ?? '',
    has_secret: Boolean(config.has_secret),
    pay_currency: config.pay_currency ?? 'CNY',
    usd_exchange_rate: Number(config.usd_exchange_rate ?? 7.2),
    min_recharge_usd: Number(config.min_recharge_usd ?? 1),
    channels: normalizeChannels(config.channels),
    created_at: config.created_at ?? null,
    updated_at: config.updated_at ?? null,
  }
}

function paymentGatewayApiFor(provider: PaymentGatewayProvider) {
  return {
    async get(): Promise<PaymentGatewayConfig> {
      const response = await apiClient.get<PaymentGatewayConfig>(`/api/admin/payments/gateways/${provider}`)
      return normalizeGatewayConfig(response.data, provider)
    },

    async update(payload: UpdatePaymentGatewayConfigRequest): Promise<PaymentGatewayConfig> {
      const request: UpdatePaymentGatewayConfigRequest = {
        ...payload,
        channels: normalizeChannels(payload.channels),
      }
      const response = await apiClient.put<PaymentGatewayConfig>(`/api/admin/payments/gateways/${provider}`, request)
      return normalizeGatewayConfig(response.data, provider)
    },

    async test(): Promise<GatewayTestResponse> {
      const response = await apiClient.post<GatewayTestResponse>(`/api/admin/payments/gateways/${provider}/test`, {})
      return response.data
    },
  }
}

export const epayGatewayApi = {
  async get(): Promise<EpayGatewayConfig> {
    return paymentGatewayApiFor('epay').get() as Promise<EpayGatewayConfig>
  },

  async update(payload: UpdateEpayGatewayConfigRequest): Promise<EpayGatewayConfig> {
    return paymentGatewayApiFor('epay').update(payload) as Promise<EpayGatewayConfig>
  },

  async test(): Promise<GatewayTestResponse> {
    return paymentGatewayApiFor('epay').test()
  },
}

export const dodopayGatewayApi = {
  async get(): Promise<DodopayGatewayConfig> {
    return paymentGatewayApiFor('dodopay').get() as Promise<DodopayGatewayConfig>
  },

  async update(payload: UpdateDodopayGatewayConfigRequest): Promise<DodopayGatewayConfig> {
    return paymentGatewayApiFor('dodopay').update(payload) as Promise<DodopayGatewayConfig>
  },

  async test(): Promise<GatewayTestResponse> {
    return paymentGatewayApiFor('dodopay').test()
  },
}

export const adminBillingPlansApi = {
  async list(): Promise<BillingPlanListResponse> {
    const response = await apiClient.get<BillingPlanListResponse>('/api/admin/billing/plans')
    return response.data
  },

  async create(payload: BillingPlanWriteRequest): Promise<BillingPlan> {
    const response = await apiClient.post<BillingPlan>('/api/admin/billing/plans', payload)
    return response.data
  },

  async update(planId: string, payload: BillingPlanWriteRequest): Promise<BillingPlan> {
    const response = await apiClient.put<BillingPlan>(`/api/admin/billing/plans/${planId}`, payload)
    return response.data
  },

  async setStatus(planId: string, enabled: boolean): Promise<BillingPlan> {
    const response = await apiClient.patch<BillingPlan>(`/api/admin/billing/plans/${planId}/status`, { enabled })
    return response.data
  },

  async delete(planId: string): Promise<void> {
    await apiClient.delete(`/api/admin/billing/plans/${planId}`)
  },
}

export const billingApi = {
  async listPlans(): Promise<BillingPlanListResponse> {
    const response = await apiClient.get<BillingPlanListResponse>('/api/billing/plans')
    return response.data
  },

  async checkout(planId: string, payload: BillingCheckoutRequest): Promise<BillingCheckoutResponse> {
    const response = await apiClient.post<BillingCheckoutResponse>(
      `/api/billing/plans/${planId}/checkout`,
      payload
    )
    return response.data
  },

  async listEntitlements(): Promise<UserPlanEntitlementsResponse> {
    const response = await apiClient.get<UserPlanEntitlementsResponse>('/api/billing/entitlements')
    return response.data
  },
}
