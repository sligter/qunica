import { afterEach, describe, expect, it, vi } from 'vitest'
import { lanFetch, LanTransportError } from './lanFetch'
import { LAN_ORIGIN } from './androidSession'
const mocks = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }))
afterEach(() => mocks.invoke.mockReset())

function transport(chunks: { end: boolean; data: string }[] = []) {
  mocks.invoke.mockImplementation(async (command: string) => {
    if (command === 'mobile_lan_prepare') return 'request-id'
    if (command === 'mobile_lan_open') return { status: 200, headers: [['content-type', 'text/event-stream']] }
    if (command === 'mobile_lan_read') return chunks.shift() ?? { end: true, data: '' }
  })
}
describe('native LAN fetch', () => {
  it('preserves SSE chunks, cursor headers and request bodies without browser fetch', async () => {
    transport([{ end: false, data: btoa('id: one\ndata: first\n\n') }, { end: false, data: btoa('id: two\ndata: second\n\n') }])
    const response = await lanFetch(`${LAN_ORIGIN}/api/v2/events?stream=one`, { method: 'POST', headers: { 'last-event-id': 'one', authorization: 'Bearer token' }, body: 'approval' })
    expect(response.headers.get('content-type')).toBe('text/event-stream')
    expect(await response.text()).toBe('id: one\ndata: first\n\nid: two\ndata: second\n\n')
    expect(mocks.invoke).toHaveBeenCalledWith('mobile_lan_open', expect.objectContaining({ head: expect.objectContaining({ path: '/api/v2/events?stream=one', headers: expect.arrayContaining([['last-event-id', 'one']]) }), body: btoa('approval').replace(/=+$/, '') }))
    expect(mocks.invoke).toHaveBeenCalledWith('mobile_lan_close', { id: 'request-id' })
  })
  it('rejects requests outside the paired API', async () => {
    await expect(lanFetch('https://example.com/api/v2/health')).rejects.toThrow('Only paired')
    await expect(lanFetch(`${LAN_ORIGIN}/index.html`)).rejects.toThrow('Only paired')
    expect(mocks.invoke).not.toHaveBeenCalled()
  })
  it('closes the native request when the streaming body is cancelled', async () => {
    transport()
    const response = await lanFetch(`${LAN_ORIGIN}/api/v2/events`)
    await response.body!.cancel()
    expect(mocks.invoke).toHaveBeenCalledWith('mobile_lan_close', { id: 'request-id' })
  })
  it('aborts a pending native response without leaving the socket open', async () => {
    const controller = new AbortController()
    let rejectOpen!: (error: Error) => void
    mocks.invoke.mockImplementation(async (command: string) => {
      if (command === 'mobile_lan_prepare') return 'request-id'
      if (command === 'mobile_lan_open') return new Promise((_resolve, reject) => { rejectOpen = reject })
      if (command === 'mobile_lan_close') rejectOpen(new Error('cancelled'))
    })
    const pending = lanFetch(`${LAN_ORIGIN}/api/v2/events`, { signal: controller.signal })
    const assertion = expect(pending).rejects.toMatchObject({ name: 'AbortError' })
    await vi.waitFor(() => expect(rejectOpen).toBeDefined())
    controller.abort()
    await assertion
    expect(mocks.invoke).toHaveBeenCalledWith('mobile_lan_close', { id: 'request-id' })
  })
  it('marks uncertain delivery so a failed mutation cannot be silently resubmitted', async () => {
    mocks.invoke.mockImplementation(async (command: string) => {
      if (command === 'mobile_lan_prepare') return 'request-id'
      if (command === 'mobile_lan_open') throw new Error('connection lost after upload')
    })
    await expect(lanFetch(`${LAN_ORIGIN}/api/v2/messages`, { method: 'POST', body: '{}' })).rejects.toBeInstanceOf(LanTransportError)
    expect(mocks.invoke.mock.calls.filter(([c]) => c === 'mobile_lan_open')).toHaveLength(1)
  })
})
