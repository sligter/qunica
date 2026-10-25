import { Blob as NodeBlob } from 'node:buffer'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { saveAndroidFile, saveAndroidResponse } from './androidFileExport'

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }))
afterEach(() => mocks.invoke.mockReset())
const blob = (bytes: Uint8Array) => new NodeBlob([bytes], { type: 'application/pdf' }) as unknown as Blob

describe('Android file export', () => {
  it('preserves binary bytes in bounded, ordered chunks and waits for the system save result', async () => {
    const bytes = Uint8Array.from({ length: 180_000 }, (_, i) => i % 256)
    const transferred: Uint8Array[] = []
    let offset = 0
    mocks.invoke.mockImplementation(async (_command, { operation, payload }) => {
      if (operation === 'begin') {
        expect(payload).toEqual({ name: '报告.pdf', mime: 'application/pdf', size: bytes.length })
        return { id: 'export-1' }
      }
      if (operation === 'append') {
        expect(payload.offset).toBe(offset)
        const chunk = Uint8Array.from(atob(payload.data), c => c.charCodeAt(0))
        expect(chunk.length).toBeLessThanOrEqual(65536)
        transferred.push(chunk)
        offset += chunk.length
      }
      if (operation === 'save') return { uri: 'content://downloads/report' }
      return {}
    })
    await expect(saveAndroidFile('报告.pdf', blob(bytes))).resolves.toBe('content://downloads/report')
    expect(Buffer.concat(transferred)).toEqual(Buffer.from(bytes))
    expect(mocks.invoke.mock.calls.at(-1)?.[1].operation).toBe('discard')
  })

  it('treats picker cancellation as a normal result and releases staging', async () => {
    mocks.invoke.mockResolvedValueOnce({ id: 'empty' }).mockResolvedValueOnce({ uri: null }).mockResolvedValue({})
    await expect(saveAndroidFile('empty.txt', blob(new Uint8Array()))).resolves.toBeNull()
    expect(mocks.invoke.mock.calls.map(c => c[1].operation)).toEqual(['begin', 'save', 'discard'])
  })

  it('discards partial data after transfer failure without retrying or opening the picker', async () => {
    mocks.invoke.mockResolvedValueOnce({ id: 'failed' }).mockRejectedValueOnce(new Error('Disk full')).mockRejectedValueOnce(new Error('Bridge gone'))
    await expect(saveAndroidFile('file.bin', blob(new Uint8Array(4)))).rejects.toThrow('Disk full')
    expect(mocks.invoke.mock.calls.map(c => c[1].operation)).toEqual(['begin', 'append', 'discard'])
  })

  it('reports save failures and still releases the staged file', async () => {
    mocks.invoke.mockResolvedValueOnce({ id: 'failed' }).mockRejectedValueOnce(new Error('Destination unavailable')).mockResolvedValue({})
    await expect(saveAndroidFile('empty', blob(new Uint8Array()))).rejects.toThrow('Destination unavailable')
    expect(mocks.invoke.mock.calls.at(-1)?.[1].operation).toBe('discard')
  })

  it('streams a response into bounded native chunks without making a whole-file Blob', async () => {
    const bytes = Uint8Array.from({ length: 200_003 }, (_, i) => i % 251)
    const response = new Response(new ReadableStream({
      start(controller) { controller.enqueue(bytes.subarray(0, 1)); controller.enqueue(bytes.subarray(1)); controller.close() },
    }), { headers: { 'Content-Type': 'application/zip' } })
    const toBlob = vi.spyOn(response, 'blob')
    const received: Uint8Array[] = []
    const progress = vi.fn()
    mocks.invoke.mockImplementation(async (_command, { operation, payload }) => {
      if (operation === 'begin') {
        expect(payload).toEqual({ name: 'results.zip', mime: 'application/zip', size: bytes.length })
        return { id: 'stream' }
      }
      if (operation === 'append') {
        expect(payload.offset).toBe(received.reduce((size, chunk) => size + chunk.length, 0))
        const chunk = Uint8Array.from(atob(payload.data), c => c.charCodeAt(0))
        expect(chunk.length).toBeLessThanOrEqual(65536)
        received.push(chunk)
      }
      if (operation === 'save') return { uri: 'content://downloads/results' }
      return {}
    })
    await expect(saveAndroidResponse('results.zip', response, bytes.length, progress)).resolves.toBe('content://downloads/results')
    expect(Buffer.concat(received)).toEqual(Buffer.from(bytes))
    expect(progress).toHaveBeenLastCalledWith(bytes.length)
    expect(toBlob).not.toHaveBeenCalled()
    expect(response.body?.locked).toBe(false)
  })

  it.each([2, 5])('rejects a response whose actual length differs from %s without opening the picker', async size => {
    mocks.invoke.mockImplementation(async (_command, { operation }) => operation === 'begin' ? { id: 'changed' } : {})
    await expect(saveAndroidResponse('changed.bin', new Response(new Uint8Array(3)), size)).rejects.toThrow(/size changed|incomplete/)
    expect(mocks.invoke.mock.calls.some(call => call[1].operation === 'save')).toBe(false)
    expect(mocks.invoke.mock.calls.at(-1)?.[1].operation).toBe('discard')
  })

  it('cancels a stalled response and discards staging when the account or download is aborted', async () => {
    const controller = new AbortController()
    const cancelled = vi.fn()
    let staged!: () => void
    const started = new Promise<void>(resolve => { staged = resolve })
    const response = new Response(new ReadableStream({ cancel: cancelled }))
    mocks.invoke.mockImplementation(async (_command, { operation }) => {
      if (operation === 'begin') { staged(); return { id: 'aborted' } }
      return {}
    })
    const saving = saveAndroidResponse('stalled.bin', response, 5, undefined, controller.signal)
    const rejected = expect(saving).rejects.toThrow(/abort/i)
    await started
    controller.abort()
    await rejected
    expect(cancelled).toHaveBeenCalledTimes(1)
    expect(mocks.invoke.mock.calls.map(call => call[1].operation)).toEqual(['begin', 'discard'])
    expect(response.body?.locked).toBe(false)
  })

  it('cancels the response even when native staging cannot start', async () => {
    const cancelled = vi.fn()
    const response = new Response(new ReadableStream({ cancel: cancelled }))
    mocks.invoke.mockRejectedValueOnce(new Error('Disk full'))
    await expect(saveAndroidResponse('full.bin', response, 5)).rejects.toThrow('Disk full')
    expect(cancelled).toHaveBeenCalled()
    expect(response.body?.locked).toBe(false)
  })
})
