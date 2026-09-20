import { describe, expect, it } from 'vitest'
import { summarizeCodexState } from '../codex-state-summary'
import type { CodexStateObservation } from '@/api/endpoints/codex-state'
const item: CodexStateObservation = {
  model: 'model', egress: 'direct', node_id: null, instance: 'app', last_seen_at: 1,
  status: 'ready', current: true, expires_at: 100, retry_until: null, auth_status: null,
  cooldown_seconds: 0, last_probe: null, last_use: null,
}
function summary(items: CodexStateObservation[]) {
  return summarizeCodexState({ read_failed: false, diagnostics: { enabled: true, observed_at: 1, items } })
}
describe('current egress summary', () => {
  it('distinguishes automatically queued accounts from failed collection', () => {
    expect(summary([{ ...item, status: 'queued' }]).status).toBe('queued')
  })
  it('lists only currently ready models and deduplicates multiple exits', () => {
    const result = summary([item, { ...item, egress: 'shared_proxy' }, { ...item, model: 'other', status: 'queued' },
      { ...item, model: 'old', status: 'credential_changed', current: false }])
    expect(result.readyModels).toEqual(['model'])
  })
  it('excludes old egress from availability and recent injection', () => {
    const result = summary([item, { ...item, status: 'egress_changed', current: false,
      last_use: { at: 9, mode: 'injected', http_status: 200 } }])
    expect(result.status).toBe('ready')
    expect(result.total).toBe(1)
    expect(result.ready).toBe(1)
    expect(result.use).toBeNull()
    expect(result.history).toHaveLength(1)
  })
  it('does not present legacy configuration changes as current account failure', () => {
    const result = summary([{ ...item, current: undefined, status: 'configuration_changed' }])
    expect(result.status).toBe('unobserved_current')
    expect(result.total).toBe(0)
  })
  it('keeps a failed collection visible during cooldown', () => {
    expect(summary([{ ...item, status: 'cooldown', cooldown_seconds: 30,
      last_probe: { at: 1, status: 0, accepted: false, reason: 'transport_error' } }]).status).toBe('collection_failed')
  })
  it('never converts rate limits or authentication rejection into collection failure', () => {
    for (const status of ['rate_limited', 'auth_rejected'] as const) {
      expect(summary([{ ...item, status,
        last_probe: { at: 1, status: 401, accepted: false, reason: 'upstream_error' } }]).status).toBe(status)
    }
  })
})

it('shows a running attempt without changing it into collection failure', () => {
  expect(summary([{ ...item, status: 'unavailable', last_probe: { at: 1, status: 0, accepted: false, reason: 'collecting', attempt: 2, attempt_limit: 6 } }]).status).toBe('collecting')
})
