export type CcSwitchApp = 'claude' | 'codex' | 'gemini'

export const DEFAULT_CCSWITCH_CODEX_MODEL = 'gpt-5.5'
export const DEFAULT_CCSWITCH_CODEX_REASONING_EFFORT = 'high'

interface BuildCcSwitchImportUrlInput {
  app: CcSwitchApp
  baseUrl: string
  providerName: string
  apiKey: string
  model?: string
}

export function normalizeCcSwitchBaseUrl(baseUrl: string): string {
  return baseUrl.trim().replace(/\/+$/, '')
}

export function ccSwitchEndpoint(app: CcSwitchApp, baseUrl: string): string {
  const normalizedBaseUrl = normalizeCcSwitchBaseUrl(baseUrl)
  return app === 'codex' ? `${normalizedBaseUrl}/v1` : normalizedBaseUrl
}

export function buildCcSwitchUsageScript(model?: string): string {
  const modelQuery = model?.trim()
    ? `?model=${encodeURIComponent(model.trim())}`
    : ''

  return `({
    request: {
      url: "{{baseUrl}}/user/balance${modelQuery}",
      method: "GET",
      headers: { "Authorization": "Bearer {{apiKey}}" }
    },
    extractor: function(response) {
      const remaining = response?.remaining ?? response?.quota?.remaining ?? response?.balance;
      const unit = response?.unit ?? response?.quota?.unit ?? "USD";
      return {
        isValid: response?.is_active ?? response?.isValid ?? true,
        remaining,
        unit
      };
    }
  })`
}

export function buildCcSwitchImportUrl(input: BuildCcSwitchImportUrlInput): string {
  const baseUrl = normalizeCcSwitchBaseUrl(input.baseUrl)
  const endpoint = ccSwitchEndpoint(input.app, baseUrl)
  const model = ccSwitchModelForImport(input.app, input.model)
  const entries: [string, string][] = [
    ['resource', 'provider'],
    ['app', input.app],
    ['name', input.providerName.trim() || 'Niffler'],
    ['homepage', baseUrl],
    ['endpoint', endpoint],
    ['apiKey', input.apiKey],
    ['enabled', 'true'],
    ['configFormat', 'json'],
    ['usageEnabled', 'true'],
    ['usageBaseUrl', baseUrl],
    ['usageScript', encodeBase64(buildCcSwitchUsageScript(model))],
    ['usageAutoInterval', '30'],
  ]

  if (model) {
    entries.splice(2, 0, ['model', model])
  }

  return `ccswitch://v1/import?${new URLSearchParams(entries).toString()}`
}

function ccSwitchModelForImport(app: CcSwitchApp, model?: string): string {
  const normalized = model?.trim() ?? ''
  if (normalized) return normalized
  return app === 'codex' ? DEFAULT_CCSWITCH_CODEX_MODEL : ''
}

function encodeBase64(value: string): string {
  const bytes = new TextEncoder().encode(value)
  let binary = ''
  for (const byte of bytes) {
    binary += String.fromCharCode(byte)
  }
  return btoa(binary)
}
