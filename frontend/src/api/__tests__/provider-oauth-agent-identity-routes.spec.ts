import { beforeEach, describe, expect, it, vi } from 'vitest'

const { getMock, postMock } = vi.hoisted(() => ({
  getMock: vi.fn(),
  postMock: vi.fn(),
}))

vi.mock('@/api/client', () => ({
  default: {
    get: getMock,
    post: postMock,
  },
}))

import {
  getBatchImportOAuthTaskStatus,
  importProviderRefreshToken,
  startBatchImportOAuthTask,
} from '@/api/endpoints/provider_oauth'

describe('Agent Identity OAuth management routes', () => {
  beforeEach(() => {
    getMock.mockReset()
    postMock.mockReset()
    getMock.mockResolvedValue({ data: {} })
    postMock.mockResolvedValue({ data: {} })
  })

  it('routes one Agent Identity JSON through the provider-oauth permission surface', async () => {
    const credentials = JSON.stringify({
      auth_mode: 'agentIdentity',
      agent_runtime_id: 'runtime-1',
      agent_private_key: 'private-key',
      task_id: 'task-1',
    })

    await startBatchImportOAuthTask('provider-codex', credentials, 'proxy-1')

    expect(postMock).toHaveBeenCalledWith(
      '/api/admin/provider-oauth/providers/provider-codex/agent-identity-import/tasks',
      { credentials, proxy_node_id: 'proxy-1' },
    )
  })

  it('routes Agent Identity arrays through the dedicated import surface', async () => {
    const credentials = JSON.stringify([
      {
        auth_mode: 'agentIdentity',
        agent_runtime_id: 'runtime-1',
        agent_private_key: 'private-key-1',
      },
      {
        auth_mode: 'agentIdentity',
        agent_runtime_id: 'runtime-2',
        agent_private_key: 'private-key-2',
      },
    ])

    await startBatchImportOAuthTask('provider-codex', credentials)

    expect(postMock).toHaveBeenCalledWith(
      '/api/admin/provider-oauth/providers/provider-codex/agent-identity-import/tasks',
      { credentials, proxy_node_id: undefined },
    )
  })

  it('routes sub2api Agent Identity exports through the dedicated import surface', async () => {
    const credentials = JSON.stringify({
      type: 'sub2api-data',
      accounts: [{
        platform: 'openai',
        credentials: {
          auth_mode: 'agentIdentity',
          agent_runtime_id: 'runtime-1',
          agent_private_key: 'private-key',
        },
      }],
    })

    await startBatchImportOAuthTask('provider-codex', credentials)

    expect(postMock).toHaveBeenCalledWith(
      '/api/admin/provider-oauth/providers/provider-codex/agent-identity-import/tasks',
      { credentials, proxy_node_id: undefined },
    )
  })

  it('routes mixed Agent Identity credentials to the dedicated surface for rejection', async () => {
    const credentials = JSON.stringify([
      { refresh_token: 'ordinary-refresh-token' },
      {
        auth_mode: 'agentIdentity',
        agent_runtime_id: 'runtime-1',
        agent_private_key: 'private-key',
      },
    ])

    await startBatchImportOAuthTask('provider-codex', credentials)

    expect(postMock).toHaveBeenCalledWith(
      '/api/admin/provider-oauth/providers/provider-codex/agent-identity-import/tasks',
      { credentials, proxy_node_id: undefined },
    )
  })

  it('keeps ordinary batch credentials on the pool permission surface', async () => {
    await startBatchImportOAuthTask('provider-codex', 'refresh-token')

    expect(postMock).toHaveBeenCalledWith(
      '/api/admin/provider-oauth/providers/provider-codex/batch-import/tasks',
      { credentials: 'refresh-token', proxy_node_id: undefined },
    )
  })

  it('polls Agent Identity tasks through the matching dedicated status route', async () => {
    await getBatchImportOAuthTaskStatus(
      'provider-codex',
      'agent-identity-task-1',
    )

    expect(getMock).toHaveBeenCalledWith(
      '/api/admin/provider-oauth/providers/provider-codex/agent-identity-import/tasks/agent-identity-task-1',
    )
  })

  it('keeps create requests on the single registration endpoint', async () => {
    await importProviderRefreshToken('provider-codex', {
      access_token: 'access-token',
      create_agent_identity: true,
    })

    expect(postMock).toHaveBeenCalledWith(
      '/api/admin/provider-oauth/providers/provider-codex/import-refresh-token',
      {
        access_token: 'access-token',
        create_agent_identity: true,
      },
    )
  })
})
