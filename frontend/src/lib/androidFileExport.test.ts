import { Blob as NodeBlob } from 'node:buffer'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { saveAndroidFile } from './androidFileExport'

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
})
