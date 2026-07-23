import { describe, expect, it } from 'vitest'

import {
  canRefreshOAuthCredential,
  getProviderAuthLabel,
  getProviderMaskedSecretLabel,
  shouldShowOAuthRefreshControl,
} from '@/utils/providerKeyAuth'

describe('providerKeyAuth', () => {
  it('renders Grok OAuth-managed cookies as sessions without OAuth refresh controls', () => {
    const key = {
      auth_type: 'oauth',
      oauth_managed: true,
      can_refresh_oauth: false,
    }

    expect(getProviderMaskedSecretLabel(key, 'grok')).toBe('[Session Cookie]')
    expect(shouldShowOAuthRefreshControl(key, 'grok')).toBe(false)
  })

  it('hides oauth refresh control when backend marks a provider as non-refreshable', () => {
    const input = {
      auth_type: 'oauth',
      oauth_managed: true,
      can_refresh_oauth: false,
    }

    expect(canRefreshOAuthCredential(input)).toBe(false)
    expect(shouldShowOAuthRefreshControl(input)).toBe(false)
  })

  it('keeps legacy oauth refresh control visible when backend capability is absent', () => {
    const input = {
      auth_type: 'oauth',
      oauth_managed: true,
    }

    expect(canRefreshOAuthCredential(input)).toBe(true)
    expect(shouldShowOAuthRefreshControl(input)).toBe(true)
    expect(getProviderMaskedSecretLabel(input, 'codex')).toBe('[OAuth Token]')
  })

  it('renders OAuth-managed authorization header credentials as OAuth Header', () => {
    const input = {
      auth_type: 'oauth',
      oauth_managed: true,
      oauth_header_auth: true,
    }

    expect(getProviderMaskedSecretLabel(input, 'codex')).toBe('[OAuth Header]')
  })

  it('renders Codex Agent Identity credentials distinctly from OAuth tokens', () => {
    const input = {
      auth_type: 'oauth',
      oauth_managed: true,
      agent_identity: true,
      can_refresh_oauth: false,
    }

    expect(getProviderMaskedSecretLabel(input, 'codex')).toBe('[Agent Identity]')
    expect(getProviderAuthLabel(input)).toBe('Agent Identity')
  })
})
