export type ManagedInstructionsReason =
  | 'applied'
  | 'already_applied'
  | 'client_instructions_present'
  | 'disabled'
  | 'unsupported_provider_api_format'

export interface ManagedInstructionsStatus {
  applied: boolean
  userGroupId: string | null
  profileId: string
  mergeMode: 'prepend' | 'if_missing'
  coreVersion: string
  profileSha256: string
  providerApiFormat: string
  targetField: string | null
  clientInstructionsPresent: boolean | null
  deduplicated: boolean
  clientMarkerPresent: boolean
  reason: ManagedInstructionsReason
}

type JsonRecord = Record<string, unknown>

const REASONS = new Set<ManagedInstructionsReason>([
  'applied',
  'already_applied',
  'client_instructions_present',
  'disabled',
  'unsupported_provider_api_format',
])

function asRecord(value: unknown): JsonRecord | null {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null
  return value as JsonRecord
}

function nonEmptyString(value: unknown): string | null {
  if (typeof value !== 'string') return null
  const trimmed = value.trim()
  return trimmed || null
}

export function normalizeManagedInstructionsStatus(value: unknown): ManagedInstructionsStatus | null {
  const source = asRecord(value)
  if (!source) return null

  const profileId = nonEmptyString(source.profile_id)
  const coreVersion = nonEmptyString(source.core_version)
  const profileSha256 = nonEmptyString(source.profile_sha256)
  const providerApiFormat = nonEmptyString(source.provider_api_format)
  const mergeMode = source.merge_mode
  const reason = source.reason
  const targetField = source.target_field === null ? null : nonEmptyString(source.target_field)
  const clientInstructionsPresent = source.client_instructions_present

  if (
    typeof source.applied !== 'boolean'
    || (typeof clientInstructionsPresent !== 'boolean' && clientInstructionsPresent !== null)
    || typeof source.deduplicated !== 'boolean'
    || typeof source.client_marker_present !== 'boolean'
    || !profileId
    || !coreVersion
    || !profileSha256
    || !/^[0-9a-f]{64}$/.test(profileSha256)
    || !providerApiFormat
    || (mergeMode !== 'prepend' && mergeMode !== 'if_missing')
    || typeof reason !== 'string'
    || !REASONS.has(reason as ManagedInstructionsReason)
    || (source.target_field !== null && !targetField)
  ) {
    return null
  }

  return {
    applied: source.applied,
    userGroupId: nonEmptyString(source.user_group_id),
    profileId,
    mergeMode,
    coreVersion,
    profileSha256,
    providerApiFormat,
    targetField,
    clientInstructionsPresent:
      reason === 'disabled' || reason === 'unsupported_provider_api_format'
        ? null
        : clientInstructionsPresent,
    deduplicated: source.deduplicated,
    clientMarkerPresent: source.client_marker_present,
    reason: reason as ManagedInstructionsReason,
  }
}
