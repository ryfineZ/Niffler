import type { CodexStateAccount, CodexStateObservation } from '@/api/endpoints/codex-state'

export function isCurrentCodexState(item: CodexStateObservation) {
  return item.current ?? !['configuration_changed', 'credential_changed', 'egress_changed', 'account_disabled', 'credential_expired'].includes(item.status)
}

export function summarizeCodexState(account: Pick<CodexStateAccount, 'diagnostics' | 'read_failed'>) {
  const data = account.diagnostics
  const all = data?.items ?? []
  const items = all.filter(isCurrentCodexState)
  const history = all.filter(item => !isCurrentCodexState(item))
  const ready = items.filter(item => item.status === 'ready').length
  const latest = (field: 'last_probe' | 'last_use') => items.reduce<CodexStateObservation | null>(
    (selected, item) => (item[field]?.at ?? 0) > (selected?.[field]?.at ?? 0) ? item : selected, null,
  )
  const failed = items.some(item => ['cooldown', 'unavailable'].includes(item.status) && item.last_probe?.accepted === false)
  function status() {
    if (account.read_failed || !data) return 'read_failed'
    if (!data.enabled) return 'disabled'
    if (ready > 0) return 'ready'
    for (const blocked of ['auth_rejected', 'rate_limited']) {
      if (items.some(item => item.status === blocked)) return blocked
    }
    if (items.some(item => item.last_probe?.reason === 'collecting')) return 'collecting'
    if (failed) return 'collection_failed'
    if (items.some(item => item.status === 'cooldown')) return 'cooldown'
    if (items.some(item => item.status === 'queued')) return 'queued'
    if (items.length) return 'unavailable'
    if (!history.length) return 'unobserved'
    if (history.every(item => item.status === 'account_disabled')) return 'account_disabled'
    if (history.some(item => item.status === 'credential_expired')) return 'credential_expired'
    return 'unobserved_current'
  }
  return {
    status: status(), ready, total: items.length, history,
    readyModels: [...new Set(items.filter(item => item.status === 'ready').map(item => item.model))],
    warnings: ready > 0 ? ['auth_rejected', 'rate_limited'].filter(status => items.some(item => item.status === status)) : [],
    probe: latest('last_probe'), use: account.read_failed ? null : latest('last_use'),
  }
}
