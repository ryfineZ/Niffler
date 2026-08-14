import { afterEach, describe, expect, it } from 'vitest'
import { createApp, type App } from '@/test/vue'

import Table from '../table.vue'

const mountedApps: Array<{ app: App, root: HTMLElement }> = []

function mountTable(className?: string) {
  const root = document.createElement('div')
  document.body.appendChild(root)

  const app = createApp(Table, { class: className })
  app.mount(root)
  mountedApps.push({ app, root })

  return root.querySelector('table')
}

afterEach(() => {
  for (const { app, root } of mountedApps.splice(0)) {
    app.unmount()
    root.remove()
  }
})

describe('Table width behavior', () => {
  it('uses the container width without forcing every table to its maximum content width', () => {
    const table = mountTable()

    expect(table).not.toBeNull()
    expect(table?.classList.contains('w-full')).toBe(true)
    expect(table?.classList.contains('min-w-max')).toBe(false)
  })

  it('keeps an explicit minimum width for pages that need horizontal scrolling', () => {
    const table = mountTable('min-w-[960px]')

    expect(table?.classList.contains('min-w-[960px]')).toBe(true)
  })
})
