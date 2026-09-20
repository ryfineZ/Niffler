import type { CodexStateAccount, CodexStateObservation } from '@/api/endpoints/codex-state'

export function summarizeCodexState(account: CodexStateAccount) {
  const data = account.diagnostics
  const items = data?.items ?? []
  const ready = items.filter(item => item.status === 'ready').length
  const latest = (field: 'last_probe' | 'last_use') => items.reduce<CodexStateObservation | null>(
    (selected, item) => (item[field]?.at ?? 0) > (selected?.[field]?.at ?? 0) ? item : selected, null,
  )
  const status = account.read_failed || !data ? 'read_failed'
    : !data.enabled ? 'disabled'
      : ready > 0 ? 'ready'
        : ['auth_rejected', 'rate_limited', 'cooldown', 'unavailable', 'configuration_changed']
          .find(status => items.some(item => item.status === status)) ?? 'unobserved'
  return {
    status, ready, total: items.length,
    warnings: ready > 0 ? ['auth_rejected', 'rate_limited'].filter(status => items.some(item => item.status === status)) : [],
    probe: latest('last_probe'), use: latest('last_use'),
  }
}
