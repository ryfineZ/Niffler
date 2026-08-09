import { describe, expect, it } from 'vitest'

import { normalizeManagedInstructionsStatus } from '../managedInstructions'

const SHA256 = '3706f70a7c8c3c2efe00343b2fd384d33baaf79b3a34c142f16c7f43e8935947'

describe('managed instructions status', () => {
  it('normalizes an applied status', () => {
    expect(normalizeManagedInstructionsStatus({
      applied: true,
      user_group_id: 'security-users',
      profile_id: 'security_research_v1',
      merge_mode: 'prepend',
      core_version: 'core_v2',
      profile_sha256: SHA256,
      provider_api_format: 'openai:responses',
      target_field: 'instructions',
      client_instructions_present: true,
      deduplicated: false,
      client_marker_present: false,
      reason: 'applied',
    })).toEqual({
      applied: true,
      userGroupId: 'security-users',
      profileId: 'security_research_v1',
      mergeMode: 'prepend',
      coreVersion: 'core_v2',
      profileSha256: SHA256,
      providerApiFormat: 'openai:responses',
      targetField: 'instructions',
      clientInstructionsPresent: true,
      deduplicated: false,
      clientMarkerPresent: false,
      reason: 'applied',
    })
  })

  it('keeps the if_missing skip reason and null target', () => {
    expect(normalizeManagedInstructionsStatus({
      applied: false,
      user_group_id: 'security-users',
      profile_id: 'security_research_v1',
      merge_mode: 'if_missing',
      core_version: 'core_v2',
      profile_sha256: SHA256,
      provider_api_format: 'openai:chat',
      target_field: null,
      client_instructions_present: true,
      deduplicated: false,
      client_marker_present: false,
      reason: 'client_instructions_present',
    })).toMatchObject({
      applied: false,
      mergeMode: 'if_missing',
      targetField: null,
      clientInstructionsPresent: true,
      deduplicated: false,
      reason: 'client_instructions_present',
    })
  })

  it.each([null, false])(
    'shows disabled and unsupported records as unchecked when the stored value is %s',
    (clientInstructionsPresent) => {
      expect(normalizeManagedInstructionsStatus({
        applied: false,
        user_group_id: 'security-users',
        profile_id: 'security_research_v1',
        merge_mode: 'prepend',
        core_version: 'core_v2',
        profile_sha256: SHA256,
        provider_api_format: 'openai:responses:compact',
        target_field: null,
        client_instructions_present: clientInstructionsPresent,
        deduplicated: false,
        client_marker_present: false,
        reason: 'unsupported_provider_api_format',
      })).toMatchObject({
        clientInstructionsPresent: null,
        reason: 'unsupported_provider_api_format',
      })
    },
  )

  it('rejects malformed records instead of showing a misleading status', () => {
    expect(normalizeManagedInstructionsStatus({
      applied: true,
      profile_id: 'security_research_v1',
      merge_mode: 'prepend',
      core_version: 'core_v2',
      profile_sha256: 'invalid',
      provider_api_format: 'openai:responses',
      target_field: 'instructions',
      client_instructions_present: false,
      deduplicated: false,
      client_marker_present: false,
      reason: 'applied',
    })).toBeNull()
  })
})
