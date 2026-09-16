export type PoolManagementStatus =
  | 'all'
  | 'active'
  | 'available'
  | 'invalid'
  | 'temporary_unavailable'
  | 'disabled'
  | 'quota_exhausted'
  | 'blocked'
export type PoolManagementSortBy = 'imported_at' | 'last_used_at' | 'score' | 'capacity'
export type PoolManagementSortOrder = 'asc' | 'desc'
export type PoolManagementStatsMode = 'current_cycle' | 'account_total'
export type PoolManagementCapacityFilter = 'all' | 'recent' | 'unresolved'

export interface PoolManagementViewState {
  providerId: string | null
  search: string
  status: PoolManagementStatus
  capacity: PoolManagementCapacityFilter
  planType: string
  page: number
  pageSize: number
  sortBy: PoolManagementSortBy | null
  sortOrder: PoolManagementSortOrder
  statsMode: PoolManagementStatsMode
}

export interface PoolManagementStateSource {
  providerId?: string
  search?: string
  status?: string
  capacity?: string
  planType?: string
  page?: string
  pageSize?: string
  sortBy?: string
  sortOrder?: string
  statsMode?: string
}

export interface StorageLike {
  getItem(key: string): string | null
  setItem(key: string, value: string): void
  removeItem(key: string): void
}

type PoolManagementViewStateInput = Partial<{
  [Key in keyof PoolManagementViewState]: unknown
}>

export const POOL_MANAGEMENT_VIEW_STORAGE_KEY = 'aether:pool-management:view-state'

export const DEFAULT_POOL_MANAGEMENT_VIEW_STATE: PoolManagementViewState = {
  providerId: null,
  search: '',
  status: 'all',
  capacity: 'all',
  planType: 'all',
  page: 1,
  pageSize: 50,
  sortBy: 'imported_at',
  sortOrder: 'desc',
  statsMode: 'current_cycle',
}

function normalizeProviderId(value: unknown): string | null {
  const normalized = String(value ?? '').trim()
  return normalized || null
}

function normalizeSearch(value: unknown): string {
  return String(value ?? '')
}

function normalizeStatus(value: unknown): PoolManagementStatus {
  if (value === 'cooldown') return 'temporary_unavailable'
  if (value === 'inactive') return 'disabled'
  if (
    value === 'active'
    || value === 'available'
    || value === 'invalid'
    || value === 'temporary_unavailable'
    || value === 'disabled'
    || value === 'quota_exhausted'
    || value === 'blocked'
  ) {
    return value
  }
  return 'all'
}

function normalizePlanType(value: unknown): string {
  const normalized = String(value ?? '').trim().toLowerCase()
  return normalized || 'all'
}

function normalizeCapacityFilter(value: unknown): PoolManagementCapacityFilter {
  return value === 'recent' || value === 'unresolved' ? value : 'all'
}

function normalizePositiveInteger(value: unknown, fallback: number): number {
  const normalized = Number.parseInt(String(value ?? ''), 10)
  if (!Number.isFinite(normalized) || normalized <= 0) {
    return fallback
  }
  return normalized
}

function normalizeSortBy(value: unknown): PoolManagementSortBy | null {
  if (value === 'imported_at' || value === 'last_used_at' || value === 'score' || value === 'capacity') {
    return value
  }
  return DEFAULT_POOL_MANAGEMENT_VIEW_STATE.sortBy
}

function normalizeSortOrder(value: unknown): PoolManagementSortOrder {
  return value === 'asc' ? 'asc' : 'desc'
}

function normalizeStatsMode(value: unknown): PoolManagementStatsMode {
  return value === 'account_total' ? 'account_total' : 'current_cycle'
}

function normalizeViewState(input: PoolManagementViewStateInput): PoolManagementViewState {
  const capacity = normalizeCapacityFilter(input.capacity)
  return {
    providerId: normalizeProviderId(input.providerId),
    search: normalizeSearch(input.search),
    status: capacity === 'all' ? normalizeStatus(input.status) : 'all',
    capacity,
    planType: normalizePlanType(input.planType),
    page: normalizePositiveInteger(input.page, DEFAULT_POOL_MANAGEMENT_VIEW_STATE.page),
    pageSize: normalizePositiveInteger(input.pageSize, DEFAULT_POOL_MANAGEMENT_VIEW_STATE.pageSize),
    sortBy: normalizeSortBy(input.sortBy),
    sortOrder: normalizeSortOrder(input.sortOrder),
    statsMode: normalizeStatsMode(input.statsMode),
  }
}

function readStoredState(storage?: StorageLike): Partial<PoolManagementViewState> {
  if (!storage) return {}

  try {
    const raw = storage.getItem(POOL_MANAGEMENT_VIEW_STORAGE_KEY)
    if (!raw) return {}
    const parsed = JSON.parse(raw) as Partial<PoolManagementViewState> | null
    return parsed && typeof parsed === 'object' ? parsed : {}
  } catch {
    return {}
  }
}

export function readPoolManagementViewState(
  source: PoolManagementStateSource,
  storage?: StorageLike,
): PoolManagementViewState {
  const stored = normalizeViewState(readStoredState(storage))
  // URL 指定任一状态筛选时，不叠加会话中保存的另一种状态。
  const capacity = source.capacity !== undefined
    ? normalizeCapacityFilter(source.capacity)
    : source.status !== undefined ? 'all' : stored.capacity
  const status = source.status !== undefined ? normalizeStatus(source.status) : stored.status

  return {
    providerId: source.providerId !== undefined ? normalizeProviderId(source.providerId) : stored.providerId,
    search: source.search !== undefined ? normalizeSearch(source.search) : stored.search,
    status: capacity === 'all' ? status : 'all',
    capacity,
    planType: source.planType !== undefined ? normalizePlanType(source.planType) : stored.planType,
    page: source.page !== undefined
      ? normalizePositiveInteger(source.page, DEFAULT_POOL_MANAGEMENT_VIEW_STATE.page)
      : stored.page,
    pageSize: source.pageSize !== undefined
      ? normalizePositiveInteger(source.pageSize, DEFAULT_POOL_MANAGEMENT_VIEW_STATE.pageSize)
      : stored.pageSize,
    sortBy: source.sortBy !== undefined ? normalizeSortBy(source.sortBy) : stored.sortBy,
    sortOrder: source.sortOrder !== undefined ? normalizeSortOrder(source.sortOrder) : stored.sortOrder,
    statsMode: source.statsMode !== undefined ? normalizeStatsMode(source.statsMode) : stored.statsMode,
  }
}

export function writePoolManagementViewState(
  state: PoolManagementViewState,
  storage?: StorageLike,
): void {
  if (!storage) return

  try {
    storage.setItem(
      POOL_MANAGEMENT_VIEW_STORAGE_KEY,
      JSON.stringify(normalizeViewState(state)),
    )
  } catch {
    // 忽略存储失败，避免影响主流程。
  }
}

export function buildPoolManagementQueryPatch(
  state: PoolManagementViewState,
): Record<string, string | undefined> {
  const normalized = normalizeViewState(state)
  const search = normalized.search.trim()
  const isDefaultSort = normalized.sortBy === DEFAULT_POOL_MANAGEMENT_VIEW_STATE.sortBy
    && normalized.sortOrder === DEFAULT_POOL_MANAGEMENT_VIEW_STATE.sortOrder

  return {
    providerId: normalized.providerId || undefined,
    search: search || undefined,
    status: normalized.status === 'all' ? undefined : normalized.status,
    capacity: normalized.capacity === 'all' ? undefined : normalized.capacity,
    planType: normalized.planType === 'all' ? undefined : normalized.planType,
    page: normalized.page <= 1 ? undefined : String(normalized.page),
    pageSize:
      normalized.pageSize === DEFAULT_POOL_MANAGEMENT_VIEW_STATE.pageSize
        ? undefined
        : String(normalized.pageSize),
    sortBy: isDefaultSort ? undefined : normalized.sortBy || undefined,
    sortOrder: isDefaultSort ? undefined : normalized.sortBy ? normalized.sortOrder : undefined,
    statsMode: normalized.statsMode === 'account_total' ? 'account_total' : undefined,
  }
}

export function resolvePoolManagementPageAfterLoad(input: {
  requestedPage: number
  pageSize: number
  total: number
}): number {
  const requestedPage = normalizePositiveInteger(
    input.requestedPage,
    DEFAULT_POOL_MANAGEMENT_VIEW_STATE.page,
  )
  const pageSize = normalizePositiveInteger(
    input.pageSize,
    DEFAULT_POOL_MANAGEMENT_VIEW_STATE.pageSize,
  )
  const total = Math.max(0, Number.parseInt(String(input.total ?? 0), 10) || 0)
  const lastPage =
    total > 0 ? Math.ceil(total / pageSize) : DEFAULT_POOL_MANAGEMENT_VIEW_STATE.page

  return Math.min(requestedPage, lastPage)
}
