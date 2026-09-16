/**
 * Public Models API - 普通用户可访问的模型列表
 */

import client from './client'
import type { TieredPricingConfig } from './endpoints/types'

export interface PublicGlobalModel {
  id: string
  name: string
  display_name: string | null
  is_active: boolean
  // 阶梯计费配置
  default_tiered_pricing: TieredPricingConfig | null
  default_price_per_request: number | null  // 按次计费价格
  // Key 能力支持
  supported_capabilities: string[] | Record<string, unknown> | null
  supports_embedding?: boolean | null
  // 模型配置（JSON）
  config: Record<string, unknown> | null
  // 调用次数
  usage_count: number
  health?: PublicModelHealth | null
}

export interface PublicModelHealth {
  status: 'healthy' | 'degraded' | 'unavailable'
  score: number | null
  active_providers: number
  active_endpoints: number
  providers: string[]
}

export interface PublicGlobalModelListResponse {
  models: PublicGlobalModel[]
  total: number
}

export interface PublicModelGroup {
  id: string
  name: string
  discount?: number
  model_discounts?: Record<string, number> | null
  sales_multiplier?: number
  model_sales_multipliers?: Record<string, number> | null
  allowed_models?: string[] | null
  allowed_models_mode?: string
}

export interface PublicModelGroupListResponse {
  groups: PublicModelGroup[]
}

export interface PublicModelGroupCatalog extends PublicModelGroup {
  models: Array<PublicGlobalModel & { health: PublicModelHealth }>
}

export interface PublicModelGroupCatalogResponse {
  groups: PublicModelGroupCatalog[]
}

/**
 * 获取公开的 GlobalModel 列表（普通用户可访问）
 */
export async function getPublicGlobalModels(params?: {
  skip?: number
  limit?: number
  is_active?: boolean
  search?: string
}): Promise<PublicGlobalModelListResponse> {
  const response = await client.get('/api/public/global-models', { params })
  return response.data
}

export async function getPublicModelGroups(): Promise<PublicModelGroupListResponse> {
  const response = await client.get('/api/public/model-groups')
  return response.data
}

export async function getPublicModelGroupCatalog(): Promise<PublicModelGroupCatalogResponse> {
  const response = await client.get('/api/public/model-groups/catalog')
  return response.data
}
