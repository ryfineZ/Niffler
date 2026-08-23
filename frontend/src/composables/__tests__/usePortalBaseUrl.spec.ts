import { describe, expect, it } from 'vitest'
import { resolvePortalOrigin } from '@/composables/usePortalBaseUrl'

describe('resolvePortalOrigin', () => {
  it('uses the configured portal canonical origin', () => {
    expect(resolvePortalOrigin('https://no3realms.com/some/path', 'https://fallback.example'))
      .toBe('https://no3realms.com')
  })

  it('falls back to the current origin for missing or invalid configuration', () => {
    expect(resolvePortalOrigin(null, 'https://no3realms.com/')).toBe('https://no3realms.com')
    expect(resolvePortalOrigin('not a url', 'https://no3realms.com/')).toBe('https://no3realms.com')
  })
})
