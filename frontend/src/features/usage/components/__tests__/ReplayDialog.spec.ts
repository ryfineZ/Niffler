import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createApp, h, nextTick, ref, type App } from '@/test/vue'
import { i18n } from '@/i18n'
import ReplayDialog from '../ReplayDialog.vue'

const mocks = vi.hoisted(() => ({ detail: vi.fn(), replay: vi.fn(), providers: vi.fn(), keys: vi.fn() }))
vi.mock('@/api/dashboard', () => ({ dashboardApi: { getRequestDetail: mocks.detail, replayRequest: mocks.replay } }))
vi.mock('@/api/endpoints/providers', () => ({ getProvidersSummary: mocks.providers }))
vi.mock('@/api/endpoints/keys', () => ({ getProviderKeys: mocks.keys }))

const apps: App[] = []
const body = { model: 'gpt-test', input: '完整历史请求', stream: true }
const result = {
  replay_id: 'replay-1', original_request_id: 'original-1', status: 'success',
  status_code: 200, provider: '测试账号', url: '/v1/responses', response_time_ms: 12,
  response_headers: {}, response_body: { output: [{ type: 'function_call', name: 'read_file' }] },
}

async function flush() { for (let i = 0; i < 8; i++) await nextTick() }
function mount() {
  const open = ref(true)
  const app = createApp({ render: () => h(ReplayDialog, {
    isOpen: open.value, requestId: 'original-1', detail: { has_request_body: true } as never,
    onClose: () => { open.value = false },
  }) })
  const root = document.createElement('div')
  document.body.append(root)
  app.mount(root)
  apps.push(app)
  return { open }
}
function send() {
  const button = [...document.querySelectorAll('button')].find(b => /发送|请求中/.test(b.textContent || ''))
  if (!button) throw new Error('未找到发送按钮')
  return button
}

beforeEach(() => {
  vi.clearAllMocks()
  i18n.global.locale.value = 'zh-CN'
  mocks.providers.mockResolvedValue({ items: [] })
  mocks.keys.mockResolvedValue([])
  mocks.detail.mockResolvedValue({ request_body: body, has_request_body: true })
  mocks.replay.mockResolvedValue(result)
})
afterEach(() => {
  apps.splice(0).forEach(app => app.unmount())
  document.body.innerHTML = ''
})

describe('请求回放', () => {
  it('打开时加载完整请求体，加载完成前不能发送', async () => {
    let resolve!: (v: unknown) => void
    mocks.detail.mockReturnValue(new Promise(r => { resolve = r }))
    mount()
    await flush()
    expect(mocks.detail).toHaveBeenCalledWith('original-1', { includeBodies: true, cacheTtlMs: 0 })
    expect(send().disabled).toBe(true)
    resolve({ request_body: body })
    await flush()
    expect(document.body.textContent).toContain('完整历史请求')
    expect(send().disabled).toBe(false)
  })

  it('发送中阻止重复发送，完成后展示成功和非文本工具输出', async () => {
    let resolve!: (v: unknown) => void
    mocks.replay.mockReturnValue(new Promise(r => { resolve = r }))
    mount()
    await flush()
    send().click()
    await flush()
    expect(send().disabled).toBe(true)
    expect(document.body.textContent).toContain('等待上游响应')
    resolve(result)
    await flush()
    expect(document.body.textContent).toContain('回放成功')
    expect(document.body.textContent).toContain('read_file')
    expect(document.body.textContent).toContain('replay-1')
  })

  it('HTTP 200 的流内错误仍显示失败并保留错误正文', async () => {
    mocks.replay.mockResolvedValue({ ...result, status: 'failed', error_message: '额度不足', response_body: { error: 'quota' } })
    mount()
    await flush()
    send().click()
    await flush()
    expect(document.body.textContent).toContain('回放失败')
    expect(document.body.textContent).toContain('额度不足')
    expect(document.body.textContent).toContain('quota')
  })

  it('请求体缺失或截断时说明原因并阻止发送', async () => {
    mocks.detail.mockResolvedValue({ request_body: { truncated: true, reason: 'usage_capture_limits_exceeded' } })
    mount()
    await flush()
    expect(send().disabled).toBe(true)
    expect(document.body.textContent).toContain('请求体不完整')
    send().click()
    expect(mocks.replay).not.toHaveBeenCalled()
  })

  it('旧后端的 dry-run 明确提示未发送，不渲染空状态码', async () => {
    mocks.replay.mockResolvedValue({ dry_run: true, url: '/v1/responses' })
    mount()
    await flush()
    send().click()
    await flush()
    expect(document.body.textContent).toContain('未实际发送')
    expect(document.body.textContent).not.toContain('回放成功')
  })

  it('关闭后完成的旧回放不会污染重新打开的窗口', async () => {
    let resolve!: (v: unknown) => void
    mocks.replay.mockReturnValue(new Promise(r => { resolve = r }))
    const { open } = mount()
    await flush()
    send().click()
    await flush()
    open.value = false
    await flush()
    open.value = true
    await flush()
    resolve(result)
    await flush()
    expect(document.body.textContent).not.toContain('read_file')
  })
  it('加载失败时可重新加载，正文就绪后才能发送', async () => {
    mocks.detail.mockRejectedValueOnce(new Error('network unavailable'))
    mount()
    await flush()
    expect(send().disabled).toBe(true)
    expect(document.body.textContent).toContain('请求体加载失败')
    const retry = [...document.querySelectorAll('button')].find(b => b.textContent?.includes('重新加载'))
    expect(retry).toBeDefined()
    retry?.click()
    await flush()
    expect(send().disabled).toBe(false)
    expect(document.body.textContent).toContain('完整历史请求')
  })

})
