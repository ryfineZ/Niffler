import { ref, watch } from 'vue'
import apiClient from '@/api/client'
import type { PortalContext } from '@/api/auth'

interface SiteInfo {
  site_name: string
  site_subtitle: string
  portal?: PortalContext
}

// 模块级缓存，所有组件共享同一份数据
const siteName = ref('Niffler')
const siteSubtitle = ref('AI Gateway')
const portal = ref<PortalContext | null>(null)
const loaded = ref(false)
let fetchPromise: Promise<void> | null = null

async function fetchSiteInfo() {
  try {
    const response = await apiClient.get<SiteInfo>('/api/public/site-info')
    siteName.value = response.data.site_name
    siteSubtitle.value = response.data.site_subtitle
    portal.value = response.data.portal ?? null
    loaded.value = true
  } catch {
    // 加载失败时保持默认值，允许后续重试
    fetchPromise = null
  }
}

async function refreshSiteInfo() {
  fetchPromise = null
  loaded.value = false
  fetchPromise = fetchSiteInfo()
  await fetchPromise
}

export function useSiteInfo() {
  if (!loaded.value && !fetchPromise) {
    fetchPromise = fetchSiteInfo()
  }
  return { siteName, siteSubtitle, portal, refreshSiteInfo }
}

// 站点名称变化时同步更新 document.title
watch(siteName, (name) => {
  document.title = name
}, { immediate: true })
