/**
 * API 错误类型定义
 * 用于替代各处的 any 类型，提供类型安全的错误处理
 */

import type { AxiosError } from 'axios'

/**
 * 后端标准错误响应格式
 */
export interface ApiErrorResponse {
  error?: {
    type?: string
    message?: string
    details?: Record<string, unknown>
  }
  detail?: string
  message?: string
  portal_mismatch?: boolean
  canonical_url?: string | null
}

/**
 * API 错误类型
 * 封装 Axios 错误，提供类型安全的错误处理
 */
export interface ApiError extends AxiosError<ApiErrorResponse> {
  response?: AxiosError<ApiErrorResponse>['response'] & {
    data?: ApiErrorResponse
  }
}

/**
 * 类型守卫：检查是否为 API 错误
 */
export function isApiError(error: unknown): error is ApiError {
  return (
    typeof error === 'object' &&
    error !== null &&
    'response' in error
  )
}

/**
 * 从错误对象中安全提取错误消息
 */
export function getErrorMessage(error: unknown, defaultMessage = '操作失败'): string {
  if (!error) return defaultMessage

  if (isApiError(error)) {
    // 优先从标准错误格式提取
    if (error.response?.data?.error?.message) {
      return error.response.data.error.message
    }
    // FastAPI 标准 detail 字段
    if (error.response?.data?.detail) {
      return error.response.data.detail
    }
    // 通用 message 字段
    if (error.response?.data?.message) {
      return error.response.data.message
    }
    // API 错误但没有可用的错误消息，返回默认消息
    // 不使用 error.message，因为那是 Axios 的默认消息如 "Request failed with status code 400"
    return defaultMessage
  }

  // 非 API 错误的 Error 实例
  if (error instanceof Error) {
    return error.message
  }

  // 字符串错误
  if (typeof error === 'string') {
    return error
  }

  return defaultMessage
}

/**
 * 从错误对象中安全提取 HTTP 状态码
 */
export function getErrorStatus(error: unknown): number | undefined {
  if (isApiError(error)) {
    return error.response?.status
  }
  return undefined
}
