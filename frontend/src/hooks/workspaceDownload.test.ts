import { afterEach, describe, expect, it, vi } from 'vitest'
import { downloadConversationWorkspaceFile } from './useConversationWorkspaceFiles'

const mocks = vi.hoisted(() => ({ fetch: vi.fn(), saveAndroid: vi.fn(), saveDesktop: vi.fn(), android: false, desktop: false }))
vi.mock('@/lib/authFetch', () => ({ authFetch: mocks.fetch }))
vi.mock('@/lib/runtime', () => ({ apiUrl: (path: string) => path }))
vi.mock('@/lib/androidSession', async importOriginal => ({
  ...await importOriginal<typeof import('@/lib/androidSession')>(), isAndroidRuntime: () => mocks.android,
}))
vi.mock('@/lib/androidFileExport', () => ({ saveAndroidFile: mocks.saveAndroid }))
vi.mock('@/lib/desktop', () => ({ isDesktopRuntime: () => mocks.desktop, saveFileViaDialog: mocks.saveDesktop }))
afterEach(() => { vi.clearAllMocks(); vi.unstubAllGlobals(); mocks.android = false; mocks.desktop = false })

describe('workspace download dispatch', () => {
  it('downloads with auth and agent scope then saves through Android instead of a blob link', async () => {
    mocks.android = true
    const data = new Blob(['%PDF-1.7'], { type: 'application/pdf' })
    mocks.fetch.mockResolvedValue({ ok: true, blob: async () => data })
    mocks.saveAndroid.mockResolvedValue('content://downloads/report')
    const createObjectURL = vi.fn()
    vi.stubGlobal('URL', { createObjectURL })
    await downloadConversationWorkspaceFile('groups', 'g1', 'docs/报告.pdf', 'token', 'agent-1')
    expect(mocks.fetch.mock.calls[0][0]).toContain('agent_id=agent-1')
    expect(mocks.fetch.mock.calls[0][1]).toMatchObject({ headers: { Authorization: 'Bearer token' } })
    expect(mocks.saveAndroid).toHaveBeenCalledWith('报告.pdf', data)
    expect(mocks.saveDesktop).not.toHaveBeenCalled()
    expect(createObjectURL).not.toHaveBeenCalled()
  })

  it('does not open the Android picker on an unsuccessful authenticated download', async () => {
    mocks.android = true
    mocks.fetch.mockResolvedValue({ ok: false, status: 403, json: async () => ({ error: { code: 'permission_denied', message: 'Denied' } }) })
    await expect(downloadConversationWorkspaceFile('direct-chats', 'c1', 'secret.txt', 'token')).rejects.toThrow()
    expect(mocks.saveAndroid).not.toHaveBeenCalled()
  })
})
