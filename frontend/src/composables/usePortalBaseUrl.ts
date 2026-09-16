import { computed } from 'vue'
import { useSiteInfo } from '@/composables/useSiteInfo'

export function resolvePortalOrigin(
  canonicalUrl: string | null | undefined,
  fallbackOrigin: string,
): string {
  const normalizedFallback = fallbackOrigin.replace(/\/+$/, '')
  if (!canonicalUrl?.trim()) return normalizedFallback

  try {
    return new URL(canonicalUrl).origin
  } catch {
    return normalizedFallback
  }
}

export function usePortalBaseUrl() {
  const { portal } = useSiteInfo()
  const portalOrigin = computed(() => resolvePortalOrigin(
    portal.value?.canonical_url,
    window.location.origin,
  ))
  const apiBaseUrl = computed(() => `${portalOrigin.value}/v1`)

  return { portalOrigin, apiBaseUrl }
}
