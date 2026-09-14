import type { PoolCapacityModel } from '@/api/endpoints/pool'

export function capacityCooldownSeconds(model: PoolCapacityModel, now: number): number {
  return Math.max(0, Math.ceil((model.cooldown_expires_at_ms - now) / 1000))
}

export function capacityModelState(model: PoolCapacityModel, now: number): PoolCapacityModel['state'] {
  if (model.state !== 'cooldown' || capacityCooldownSeconds(model, now) > 0) return model.state
  return model.last_success_at_ms != null && model.last_success_at_ms > model.last_occurred_at_ms
    ? 'recovered' : 'pending'
}
