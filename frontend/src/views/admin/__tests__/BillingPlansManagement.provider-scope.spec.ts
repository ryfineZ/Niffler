import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

import { describe, expect, it } from 'vitest'

const componentPath = resolve(process.cwd(), 'src/views/admin/BillingPlansManagement.vue')

describe('BillingPlansManagement provider scope', () => {
  it('saves providers and only previews their current models', () => {
    const source = readFileSync(componentPath, 'utf8')

    expect(source).toContain('v-model="form.allowed_provider_ids"')
    expect(source).toContain('allowed_provider_ids: form.allowed_provider_ids')
    expect(source).toContain('const derivedGlobalModels = computed')
    expect(source).not.toContain('v-model="form.allowed_global_model_ids"')
    expect(source).not.toContain('allowed_global_model_ids: form.allowed_global_model_ids')
    expect(source).not.toContain('v-model="form.allow_wallet_overage"')
    expect(source).toContain('allow_wallet_overage: true')
  })
})
