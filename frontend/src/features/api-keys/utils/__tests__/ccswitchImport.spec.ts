import { describe, expect, it } from 'vitest'

import {
  buildCcSwitchImportUrl,
  buildCcSwitchUsageScript,
} from '@/features/api-keys/utils/ccswitchImport'

function paramsFromDeeplink(value: string): URLSearchParams {
  return new URLSearchParams(value.replace('ccswitch://v1/import?', ''))
}

function decodeBase64(value: string): string {
  return Buffer.from(value, 'base64').toString('utf8')
}

describe('ccswitchImport', () => {
  it('builds a Codex provider import link with OpenAI v1 endpoint', () => {
    const deeplink = buildCcSwitchImportUrl({
      app: 'codex',
      baseUrl: 'https://niffler.example.com/',
      providerName: 'Niffler',
      apiKey: 'sk-test',
      model: 'gpt-5.4',
    })

    const params = paramsFromDeeplink(deeplink)
    expect(params.get('resource')).toBe('provider')
    expect(params.get('app')).toBe('codex')
    expect(params.get('name')).toBe('Niffler')
    expect(params.get('homepage')).toBe('https://niffler.example.com')
    expect(params.get('endpoint')).toBe('https://niffler.example.com/v1')
    expect(params.get('apiKey')).toBe('sk-test')
    expect(params.get('model')).toBe('gpt-5.4')
    expect(params.get('configFormat')).toBe('json')
    expect(params.has('config')).toBe(false)
    expect(params.get('usageEnabled')).toBe('true')
    expect(params.get('usageBaseUrl')).toBe('https://niffler.example.com')

    const usageScript = decodeBase64(params.get('usageScript') ?? '')
    expect(usageScript).toContain('{{baseUrl}}/user/balance?model=gpt-5.4')
    expect(usageScript).not.toContain('{{baseUrl}}/v1/user/balance')
  })

  it('defaults Codex imports to gpt-5.5 without a custom config payload', () => {
    const deeplink = buildCcSwitchImportUrl({
      app: 'codex',
      baseUrl: 'https://niffler.example.com',
      providerName: 'Niffler',
      apiKey: 'sk-test',
    })

    const params = paramsFromDeeplink(deeplink)
    expect(params.get('model')).toBe('gpt-5.5')
    expect(params.has('config')).toBe(false)

    const usageScript = decodeBase64(params.get('usageScript') ?? '')
    expect(usageScript).toContain('{{baseUrl}}/user/balance?model=gpt-5.5')
  })

  it('builds a Claude provider import link with usage check script', () => {
    const deeplink = buildCcSwitchImportUrl({
      app: 'claude',
      baseUrl: 'https://niffler.example.com',
      providerName: 'Niffler',
      apiKey: 'sk-test',
    })

    const params = paramsFromDeeplink(deeplink)
    expect(params.get('app')).toBe('claude')
    expect(params.get('endpoint')).toBe('https://niffler.example.com')
    expect(params.get('usageBaseUrl')).toBe('https://niffler.example.com')

    const usageScript = decodeBase64(params.get('usageScript') ?? '')
    expect(usageScript).toContain('{{baseUrl}}/user/balance')
    expect(usageScript).toContain('Authorization')
    expect(usageScript).toContain('Bearer {{apiKey}}')
  })

  it('builds usage script that reads remaining balance and key validity', () => {
    const usageScript = buildCcSwitchUsageScript()

    expect(usageScript).toContain('response?.remaining')
    expect(usageScript).toContain('response?.quota?.remaining')
    expect(usageScript).toContain('response?.is_active')
    expect(usageScript).toContain('response?.isValid')
  })
})
