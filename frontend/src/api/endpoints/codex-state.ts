import client from '../client'

export interface CodexStateObservation {
  model: string
  egress: 'direct' | 'local_proxy' | 'shared_proxy'
  node_id: string | null
  instance: string | null
  last_seen_at: number
  status: 'disabled' | 'configuration_changed' | 'rate_limited' | 'auth_rejected' | 'ready' | 'cooldown' | 'unavailable'
  expires_at: number | null
  retry_until: number | null
  auth_status: number | null
  cooldown_seconds: number
  last_probe: { at: number; status: number; accepted: boolean; reason: string | null } | null
  last_use: { at: number; mode: 'injected' | 'passthrough' | 'invalidated'; http_status: number } | null
}

export interface CodexStateDiagnostics {
  enabled: boolean
  observed_at: number
  items: CodexStateObservation[]
}

export interface CodexStateAccount {
  provider_id: string
  provider_name: string
  key_id: string
  key_name: string
  active: boolean
  read_failed: boolean
  diagnostics: CodexStateDiagnostics | null
}

export interface CodexStateOverview {
  total: number
  page: number
  page_size: number
  accounts: CodexStateAccount[]
}

export async function getCodexStateOverview(params: { page: number; page_size: number; search: string }): Promise<CodexStateOverview> {
  const response = await client.get<CodexStateOverview>('/api/admin/providers/turn-state', { params })
  return response.data
}

export async function getCodexStateDiagnostics(providerId: string, keyId: string, options?: { signal?: AbortSignal }): Promise<CodexStateDiagnostics> {
  const response = await client.get<CodexStateDiagnostics>(
    `/api/admin/providers/${encodeURIComponent(providerId)}/turn-state`,
    { params: { key_id: keyId }, signal: options?.signal },
  )
  return response.data
}
