import { beforeEach, describe, expect, it } from 'vitest'

import {
  buildPoolManagementQueryPatch,
  DEFAULT_POOL_MANAGEMENT_VIEW_STATE,
  readPoolManagementViewState,
  resolvePoolManagementPageAfterLoad,
  writePoolManagementViewState,
} from '@/features/pool/utils/poolManagementState'

function createMemoryStorage() {
  const store = new Map<string, string>()
  return {
    getItem(key: string) {
      return store.get(key) ?? null
    },
    setItem(key: string, value: string) {
      store.set(key, value)
    },
    removeItem(key: string) {
      store.delete(key)
    },
  }
}

describe('poolManagementState', () => {
  let storage: ReturnType<typeof createMemoryStorage>

  beforeEach(() => {
    storage = createMemoryStorage()
  })

  it('restores provider, filters and paging from query first', () => {
    writePoolManagementViewState(
      {
        providerId: 'provider-a',
        search: 'stored search',
        status: 'cooldown',
        capacity: 'all',
        planType: 'plus',
        page: 5,
        pageSize: 20,
        sortBy: 'last_used_at',
        sortOrder: 'asc',
        statsMode: 'account_total',
      },
      storage,
    )

    const state = readPoolManagementViewState(
      {
        providerId: 'provider-b',
        search: 'query search',
        status: 'inactive',
        capacity: 'all',
        planType: 'team',
        page: '3',
        pageSize: '100',
        sortBy: 'imported_at',
        sortOrder: 'desc',
        statsMode: 'current_cycle',
      },
      storage,
    )

    expect(state).toEqual({
      providerId: 'provider-b',
      search: 'query search',
      status: 'disabled',
      capacity: 'all',
      planType: 'team',
      page: 3,
      pageSize: 100,
      sortBy: 'imported_at',
      sortOrder: 'desc',
      statsMode: 'current_cycle',
    })
  })

  it('falls back to storage when query is missing', () => {
    writePoolManagementViewState(
      {
        providerId: 'provider-c',
        search: 'stored only',
        status: 'active',
        capacity: 'all',
        planType: 'plus',
        page: 2,
        pageSize: 50,
        sortBy: 'last_used_at',
        sortOrder: 'asc',
        statsMode: 'account_total',
      },
      storage,
    )

    const state = readPoolManagementViewState({}, storage)

    expect(state).toEqual({
      providerId: 'provider-c',
      search: 'stored only',
      status: 'active',
      capacity: 'all',
      planType: 'plus',
      page: 2,
      pageSize: 50,
      sortBy: 'last_used_at',
      sortOrder: 'asc',
      statsMode: 'account_total',
    })
  })

  it('supports score sort in storage and query state', () => {
    writePoolManagementViewState(
      {
        providerId: 'provider-g',
        search: 'score search',
        status: 'cooldown',
        capacity: 'all',
        planType: 'all',
        page: 3,
        pageSize: 25,
        sortBy: 'score',
        sortOrder: 'desc',
        statsMode: 'current_cycle',
      },
      storage,
    )

    expect(readPoolManagementViewState({}, storage)).toMatchObject({
      providerId: 'provider-g',
      search: 'score search',
      status: 'temporary_unavailable',
      capacity: 'all',
      planType: 'all',
      page: 3,
      pageSize: 25,
      sortBy: 'score',
      sortOrder: 'desc',
      statsMode: 'current_cycle',
    })

    expect(
      buildPoolManagementQueryPatch({
        providerId: 'provider-g',
        search: 'score search',
        status: 'cooldown',
        capacity: 'all',
        planType: 'plus',
        page: 3,
        pageSize: 25,
        sortBy: 'score',
        sortOrder: 'desc',
        statsMode: 'current_cycle',
      }),
    ).toMatchObject({
      planType: 'plus',
      sortBy: 'score',
      sortOrder: 'desc',
    })
  })

  it('omits defaults when building query patch', () => {
    expect(
      buildPoolManagementQueryPatch({
        providerId: 'provider-d',
        search: '  ',
        status: 'all',
        capacity: 'all',
        planType: 'all',
        page: 1,
        pageSize: 50,
        sortBy: 'imported_at',
        sortOrder: 'desc',
        statsMode: 'current_cycle',
      }),
    ).toEqual({
      providerId: 'provider-d',
      search: undefined,
      status: undefined,
      capacity: undefined,
      planType: undefined,
      page: undefined,
      pageSize: undefined,
      sortBy: undefined,
      sortOrder: undefined,
      statsMode: undefined,
    })
  })

  it('keeps sortable column state in query patch', () => {
    expect(
      buildPoolManagementQueryPatch({
        providerId: 'provider-e',
        search: '',
        status: 'all',
        capacity: 'all',
        planType: 'all',
        page: 1,
        pageSize: 50,
        sortBy: 'score',
        sortOrder: 'desc',
        statsMode: 'account_total',
      }),
    ).toMatchObject({
      sortBy: 'score',
      sortOrder: 'desc',
      statsMode: 'account_total',
    })
  })

  it('restores stats mode from storage and lets query override it', () => {
    writePoolManagementViewState(
      {
        providerId: 'provider-f',
        search: '',
        status: 'all',
        capacity: 'all',
        planType: 'all',
        page: 1,
        pageSize: 50,
        sortBy: null,
        sortOrder: 'desc',
        statsMode: 'account_total',
      },
      storage,
    )

    expect(readPoolManagementViewState({}, storage).statsMode).toBe('account_total')
    expect(
      readPoolManagementViewState({ statsMode: 'current_cycle' }, storage).statsMode,
    ).toBe('current_cycle')
  })

  it.each(['recent', 'unresolved'] as const)('restores %s capacity filters from query and storage', (capacity) => {
    const state = {
      ...DEFAULT_POOL_MANAGEMENT_VIEW_STATE,
      providerId: 'provider-a',
      capacity,
      sortBy: 'capacity' as const,
    }
    writePoolManagementViewState(state, storage)

    expect(readPoolManagementViewState({}, storage)).toEqual(state)
    expect(readPoolManagementViewState({ capacity })).toMatchObject({ capacity, status: 'all' })
    expect(buildPoolManagementQueryPatch(state)).toMatchObject({
      capacity, status: undefined, sortBy: 'capacity', sortOrder: 'desc',
    })
  })

  it('keeps URL status and capacity mutually exclusive with stored filters', () => {
    writePoolManagementViewState({ ...DEFAULT_POOL_MANAGEMENT_VIEW_STATE, capacity: 'unresolved' }, storage)
    expect(readPoolManagementViewState({ status: 'available' }, storage)).toMatchObject({
      status: 'available', capacity: 'all',
    })
    expect(readPoolManagementViewState({ capacity: 'all' }, storage).capacity).toBe('all')

    writePoolManagementViewState({ ...DEFAULT_POOL_MANAGEMENT_VIEW_STATE, status: 'disabled' }, storage)
    expect(readPoolManagementViewState({ capacity: 'recent' }, storage)).toMatchObject({
      status: 'all', capacity: 'recent',
    })
    expect(readPoolManagementViewState({ status: 'disabled', capacity: 'recent' })).toMatchObject({
      status: 'all', capacity: 'recent',
    })
    expect(readPoolManagementViewState({ capacity: 'invalid' }).capacity).toBe('all')
  })

  it('clamps a restored page to the last available page after load', () => {
    expect(
      resolvePoolManagementPageAfterLoad({
        requestedPage: 5,
        pageSize: 50,
        total: 120,
      }),
    ).toBe(3)
  })

  it('resets an out-of-range empty result page back to page 1', () => {
    expect(
      resolvePoolManagementPageAfterLoad({
        requestedPage: 4,
        pageSize: 50,
        total: 0,
      }),
    ).toBe(1)
  })
})
