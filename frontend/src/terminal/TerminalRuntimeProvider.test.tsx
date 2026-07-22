import { act, cleanup, render, waitFor } from '@testing-library/react'
import { StrictMode, useEffect, type ReactNode } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import {
  TerminalRuntimeProvider,
  useTerminalRuntime,
  type TerminalRuntimeContextValue,
} from './TerminalRuntimeProvider'
import {
  TERMINAL_METADATA_STORAGE_KEY,
  type TerminalMetadataStore,
} from './metadataStore'
import { TerminalTransportError, type TerminalTransport } from './transport'
import type {
  CreateTerminalRequest,
  TerminalConversationTarget,
  TerminalDescriptor,
  TerminalEvent,
} from './types'

interface CapturedCreate {
  request: CreateTerminalRequest
  onEvent: (event: TerminalEvent) => void
}

class FakeTransport implements TerminalTransport {
  readonly creates: CapturedCreate[] = []
  readonly create = vi.fn(
    async (
      request: CreateTerminalRequest,
      onEvent: (event: TerminalEvent) => void,
    ): Promise<TerminalDescriptor> => {
      this.creates.push({ request, onEvent })
      return {
        sessionId: `session-${this.creates.length}`,
        shellName: `Shell ${this.creates.length}`,
        cwd: request.cwd,
      }
    },
  )
  readonly write = vi.fn(async (): Promise<void> => undefined)
  readonly resize = vi.fn(async (): Promise<void> => undefined)
  readonly close = vi.fn(async (): Promise<void> => undefined)
  readonly closeAll = vi.fn(async (): Promise<void> => undefined)
}

let runtime: TerminalRuntimeContextValue

function Probe({ target }: { target?: TerminalConversationTarget }) {
  runtime = useTerminalRuntime()
  const { registerConversation } = runtime
  const conversationId = target?.conversationId
  const availability = target?.availability
  const cwd = target?.availability === 'ready' ? target.cwd : undefined

  useEffect(() => {
    if (target === undefined) return
    return registerConversation(target)
  }, [availability, conversationId, cwd, registerConversation, target])

  return null
}

function Harness({
  transport,
  target,
  children,
}: {
  transport: TerminalTransport
  target?: TerminalConversationTarget
  children?: ReactNode
}) {
  return (
    <TerminalRuntimeProvider transport={transport}>
      <Probe target={target} />
      {children}
    </TerminalRuntimeProvider>
  )
}

function ready(conversationId: string, cwd: string): TerminalConversationTarget {
  return { conversationId, availability: 'ready', cwd }
}

function seedMetadata(metadata: TerminalMetadataStore): void {
  localStorage.setItem(TERMINAL_METADATA_STORAGE_KEY, JSON.stringify(metadata))
}

function storedMetadata(): TerminalMetadataStore {
  return JSON.parse(localStorage.getItem(TERMINAL_METADATA_STORAGE_KEY) ?? 'null')
}

async function toggleDock(): Promise<void> {
  await act(async () => runtime.toggleDock())
}

async function createTab(): Promise<void> {
  await act(async () => runtime.createTab())
}

describe('TerminalRuntimeProvider', () => {
  beforeEach(() => {
    localStorage.clear()
  })

  afterEach(() => {
    cleanup()
    vi.restoreAllMocks()
  })

  it('keeps one conversation running while another becomes active', async () => {
    const transport = new FakeTransport()
    const { rerender } = render(
      <Harness transport={transport} target={ready('chat-a', 'D:/a')} />,
    )
    await toggleDock()

    rerender(<Harness transport={transport} target={ready('chat-b', 'D:/b')} />)

    expect(transport.create).toHaveBeenCalledTimes(1)
    expect(transport.close).not.toHaveBeenCalled()
    expect(runtime.activeConversation).toEqual(ready('chat-b', 'D:/b'))
    expect(runtime.allTabs).toHaveLength(1)
    expect(runtime.allTabs[0]?.conversationId).toBe('chat-a')
  })

  it('does not create a terminal without a registered ready conversation', async () => {
    const transport = new FakeTransport()
    render(<Harness transport={transport} />)

    await act(async () => runtime.toggleDock())
    await act(async () => runtime.createTab())

    expect(transport.create).not.toHaveBeenCalled()
    expect(runtime.activeConversation).toBeNull()
  })

  it('restores every saved tab once and falls back from a failed absolute cwd', async () => {
    seedMetadata({
      height: 321,
      conversations: {
        'chat-a': {
          open: true,
          activeTabId: 'tab-b',
          tabs: [
            { id: 'tab-a', label: 'PowerShell', launchDirectory: 'D:/a' },
            { id: 'tab-b', label: 'Dev server', launchDirectory: 'D:/missing' },
          ],
        },
      },
    })
    const transport = new FakeTransport()
    transport.create.mockImplementation(async (request, onEvent) => {
      transport.creates.push({ request, onEvent })
      if (request.cwd === 'D:/missing') {
        throw new TerminalTransportError('terminal.invalid_cwd', 'missing')
      }
      return {
        sessionId: `session-${transport.creates.length}`,
        shellName: 'PowerShell',
        cwd: request.cwd,
      }
    })

    const { rerender } = render(
      <Harness transport={transport} target={ready('chat-a', 'D:/a')} />,
    )
    await waitFor(() => expect(runtime.activeTabs.every((tab) => tab.status === 'running')).toBe(true))
    rerender(<Harness transport={transport} target={ready('chat-a', 'D:/a')} />)

    expect(transport.create.mock.calls.map(([request]) => request.cwd)).toEqual([
      'D:/a',
      'D:/missing',
      'D:/a',
    ])
    expect(runtime.activeTabs).toHaveLength(2)
    expect(runtime.activeTabId).toBe('tab-b')
    expect(runtime.activeTabs.find((tab) => tab.tabId === 'tab-b')?.launchDirectory).toBe('D:/a')
    expect(storedMetadata().conversations['chat-a']?.tabs[1]?.launchDirectory).toBe('D:/a')
  })

  it('ignores late events from a failed cwd attempt after its fallback starts', async () => {
    seedMetadata({
      height: 0,
      conversations: {
        chat: {
          open: true,
          activeTabId: 'tab',
          tabs: [{ id: 'tab', label: 'Shell', launchDirectory: 'D:/missing' }],
        },
      },
    })
    const transport = new FakeTransport()
    transport.create.mockImplementation(async (request, onEvent) => {
      transport.creates.push({ request, onEvent })
      if (request.cwd === 'D:/missing') throw new Error('missing')
      return { sessionId: 'fallback', shellName: 'Shell', cwd: request.cwd }
    })
    render(<Harness transport={transport} target={ready('chat', 'D:/workspace')} />)
    await waitFor(() => expect(runtime.activeTabs[0]?.status).toBe('running'))
    const output = vi.fn()
    runtime.subscribeOutput('tab', output)

    act(() => {
      transport.creates[0]?.onEvent({
        event: 'error',
        data: { code: 'late', message: 'late failure' },
      })
      transport.creates[0]?.onEvent({
        event: 'output',
        data: { bytes: new Uint8Array([1]) },
      })
    })

    expect(runtime.activeTabs[0]).toMatchObject({ status: 'running', sessionId: 'fallback' })
    expect(output).not.toHaveBeenCalled()
  })

  it('uses the ready cwd directly when saved metadata is not absolute', async () => {
    seedMetadata({
      height: 0,
      conversations: {
        chat: {
          open: true,
          activeTabId: 'tab-a',
          tabs: [{ id: 'tab-a', label: 'Shell', launchDirectory: 'relative/path' }],
        },
      },
    })
    const transport = new FakeTransport()

    render(<Harness transport={transport} target={ready('chat', 'D:/workspace')} />)
    await waitFor(() => expect(runtime.activeTabs[0]?.status).toBe('running'))

    expect(transport.create).toHaveBeenCalledOnce()
    expect(transport.create.mock.calls[0]?.[0].cwd).toBe('D:/workspace')
    expect(storedMetadata().conversations.chat?.tabs[0]?.launchDirectory).toBe('D:/workspace')
  })

  it('repairs a missing active tab to the first restored tab', async () => {
    seedMetadata({
      height: 0,
      conversations: {
        chat: {
          open: true,
          activeTabId: null,
          tabs: [
            { id: 'first', label: 'First', launchDirectory: 'D:/workspace' },
            { id: 'second', label: 'Second', launchDirectory: 'D:/workspace' },
          ],
        },
      },
    })
    const transport = new FakeTransport()
    render(<Harness transport={transport} target={ready('chat', 'D:/workspace')} />)

    await waitFor(() => expect(runtime.activeTabs).toHaveLength(2))

    expect(runtime.activeTabId).toBe('first')
    expect(storedMetadata().conversations.chat?.activeTabId).toBe('first')
  })

  it('captures synchronous create output and flushes it in order to the first subscriber', async () => {
    const transport = new FakeTransport()
    transport.create.mockImplementation(async (request, onEvent) => {
      transport.creates.push({ request, onEvent })
      onEvent({ event: 'output', data: { bytes: new Uint8Array([1, 2]) } })
      onEvent({ event: 'output', data: { bytes: new Uint8Array([3]) } })
      return { sessionId: 'sync-session', shellName: 'Shell', cwd: request.cwd }
    })
    render(<Harness transport={transport} target={ready('chat', 'D:/workspace')} />)
    await toggleDock()
    const chunks: number[][] = []

    const unsubscribe = runtime.subscribeOutput(
      runtime.activeTabId!,
      (bytes) => chunks.push(Array.from(bytes)),
    )

    expect(chunks).toEqual([[1, 2], [3]])
    unsubscribe()
  })

  it('buffers only the latest 256 KiB before the first subscriber and never buffers again', async () => {
    const transport = new FakeTransport()
    transport.create.mockImplementation(async (request, onEvent) => {
      transport.creates.push({ request, onEvent })
      onEvent({ event: 'output', data: { bytes: new Uint8Array(200 * 1024).fill(1) } })
      onEvent({ event: 'output', data: { bytes: new Uint8Array(100 * 1024).fill(2) } })
      return { sessionId: 'large-session', shellName: 'Shell', cwd: request.cwd }
    })
    render(<Harness transport={transport} target={ready('chat', 'D:/workspace')} />)
    await toggleDock()
    const chunks: Uint8Array[] = []
    const tabId = runtime.activeTabId!

    const unsubscribe = runtime.subscribeOutput(tabId, (bytes) => chunks.push(bytes))
    expect(chunks.reduce((total, chunk) => total + chunk.byteLength, 0)).toBe(256 * 1024)
    expect(chunks[0]?.every((byte) => byte === 1)).toBe(true)
    expect(chunks.at(-1)?.every((byte) => byte === 2)).toBe(true)
    unsubscribe()
    transport.creates[0]?.onEvent({ event: 'output', data: { bytes: new Uint8Array([9]) } })
    const later: number[][] = []
    runtime.subscribeOutput(tabId, (bytes) => later.push(Array.from(bytes)))
    expect(later).toEqual([])
  })

  it('routes output to the matching tab and maps exit and error events', async () => {
    const transport = new FakeTransport()
    render(<Harness transport={transport} target={ready('chat', 'D:/workspace')} />)
    await toggleDock()
    await createTab()
    const [first, second] = runtime.activeTabs
    const firstOutput: number[][] = []
    const secondOutput: number[][] = []
    runtime.subscribeOutput(first!.tabId, (bytes) => firstOutput.push(Array.from(bytes)))
    runtime.subscribeOutput(second!.tabId, (bytes) => secondOutput.push(Array.from(bytes)))

    act(() => {
      transport.creates[0]?.onEvent({ event: 'output', data: { bytes: new Uint8Array([1]) } })
      transport.creates[1]?.onEvent({ event: 'output', data: { bytes: new Uint8Array([2]) } })
      transport.creates[0]?.onEvent({ event: 'exit', data: { code: 7, signal: null } })
      transport.creates[1]?.onEvent({
        event: 'error',
        data: { code: 'terminal.read_failed', message: 'read failed' },
      })
    })

    expect(firstOutput).toEqual([[1]])
    expect(secondOutput).toEqual([[2]])
    expect(runtime.allTabs.find((tab) => tab.tabId === first!.tabId)).toMatchObject({
      status: 'exited',
      exitCode: 7,
    })
    expect(runtime.allTabs.find((tab) => tab.tabId === second!.tabId)).toMatchObject({
      status: 'error',
      error: { code: 'terminal.read_failed' },
    })
  })

  it('restarts with a new generation and ignores late events from the old session', async () => {
    const transport = new FakeTransport()
    render(<Harness transport={transport} target={ready('chat', 'D:/workspace')} />)
    await toggleDock()
    const tabId = runtime.activeTabId!
    const oldEvent = transport.creates[0]!.onEvent

    await act(async () => runtime.restartTab(tabId))
    act(() => {
      oldEvent({ event: 'error', data: { code: 'late', message: 'late event' } })
      oldEvent({ event: 'output', data: { bytes: new Uint8Array([9]) } })
    })

    expect(transport.close).toHaveBeenCalledWith('chat', 'session-1')
    expect(transport.create).toHaveBeenCalledTimes(2)
    expect(runtime.activeTabs[0]).toMatchObject({ sessionId: 'session-2', status: 'running' })
  })

  it('keeps the old session when restart cleanup fails and creates only after a retry', async () => {
    const transport = new FakeTransport()
    transport.close.mockRejectedValueOnce(new Error('cleanup failed'))
    render(<Harness transport={transport} target={ready('chat', 'D:/workspace')} />)
    await toggleDock()
    const tabId = runtime.activeTabId!
    const oldEvent = transport.creates[0]!.onEvent

    let restartError: unknown
    await act(async () => {
      try {
        await runtime.restartTab(tabId)
      } catch (cause) {
        restartError = cause
      }
    })

    expect(restartError).toMatchObject({ code: 'terminal.cleanup_failed' })
    expect(transport.create).toHaveBeenCalledOnce()
    expect(runtime.activeTabs[0]).toMatchObject({
      sessionId: 'session-1',
      status: 'error',
      error: { code: 'terminal.cleanup_failed' },
    })
    act(() => {
      oldEvent({ event: 'exit', data: { code: 0, signal: null } })
      oldEvent({ event: 'error', data: { code: 'late', message: 'late' } })
    })
    expect(runtime.activeTabs[0]).toMatchObject({
      sessionId: 'session-1',
      status: 'error',
      error: { code: 'terminal.cleanup_failed' },
    })

    await act(async () => runtime.restartTab(tabId))

    expect(transport.close).toHaveBeenCalledTimes(2)
    expect(transport.create).toHaveBeenCalledTimes(2)
    expect(runtime.activeTabs[0]).toMatchObject({
      sessionId: 'session-2',
      status: 'running',
      error: null,
    })
  })

  it('does not recreate when closeTab cancels a restart waiting on native close', async () => {
    let releaseClose: (() => void) | undefined
    const closeGate = new Promise<void>((resolve) => {
      releaseClose = resolve
    })
    const transport = new FakeTransport()
    transport.close.mockImplementation(() => closeGate)
    render(<Harness transport={transport} target={ready('chat', 'D:/workspace')} />)
    await toggleDock()
    const tabId = runtime.activeTabId!

    const restarting = runtime.restartTab(tabId)
    const closing = runtime.closeTab(tabId)
    releaseClose?.()
    await act(async () => Promise.all([restarting, closing]))

    expect(transport.create).toHaveBeenCalledOnce()
    expect(runtime.allTabs).toHaveLength(0)
  })

  it('does not recreate when closeAll cancels a restart waiting on native close', async () => {
    let releaseClose: (() => void) | undefined
    const closeGate = new Promise<void>((resolve) => {
      releaseClose = resolve
    })
    const transport = new FakeTransport()
    transport.close.mockImplementation(() => closeGate)
    render(<Harness transport={transport} target={ready('chat', 'D:/workspace')} />)
    await toggleDock()
    const tabId = runtime.activeTabId!

    const restarting = runtime.restartTab(tabId)
    await act(async () => runtime.closeAll(true))
    releaseClose?.()
    await act(async () => restarting)

    expect(transport.create).toHaveBeenCalledOnce()
    expect(runtime.allTabs).toHaveLength(0)
  })

  it('waits for a pending create and closes its orphan before closeTab resolves', async () => {
    let resolveCreate: ((descriptor: TerminalDescriptor) => void) | undefined
    const deferred = new Promise<TerminalDescriptor>((resolve) => {
      resolveCreate = resolve
    })
    let releaseOrphanClose: (() => void) | undefined
    const orphanCloseGate = new Promise<void>((resolve) => {
      releaseOrphanClose = resolve
    })
    const transport = new FakeTransport()
    transport.create.mockImplementation((request, onEvent) => {
      transport.creates.push({ request, onEvent })
      return deferred
    })
    transport.close.mockImplementation(() => orphanCloseGate)
    render(<Harness transport={transport} target={ready('chat', 'D:/workspace')} />)
    let opening!: Promise<void>
    act(() => {
      opening = runtime.toggleDock()
    })
    await waitFor(() => expect(runtime.activeTabs).toHaveLength(1))
    const tabId = runtime.activeTabId!

    let closeSettled = false
    const closing = runtime.closeTab(tabId).finally(() => {
      closeSettled = true
    })
    await act(async () => Promise.resolve())

    expect(closeSettled).toBe(false)
    expect(transport.close).not.toHaveBeenCalled()

    resolveCreate?.({ sessionId: 'orphan', shellName: 'Shell', cwd: 'D:/workspace' })
    await waitFor(() => expect(transport.close).toHaveBeenCalledWith('chat', 'orphan'))
    expect(closeSettled).toBe(false)
    releaseOrphanClose?.()
    await act(async () => Promise.all([opening, closing]))

    expect(transport.close).toHaveBeenCalledWith('chat', 'orphan')
    expect(closeSettled).toBe(true)
    expect(runtime.activeTabs).toHaveLength(0)
  })

  it('removes a closed tab in finally and remains idempotent when native close fails', async () => {
    const transport = new FakeTransport()
    transport.close.mockRejectedValueOnce(new Error('close failed'))
    render(<Harness transport={transport} target={ready('chat', 'D:/workspace')} />)
    await toggleDock()
    const tabId = runtime.activeTabId!

    await expect(act(async () => runtime.closeTab(tabId))).rejects.toThrow('close failed')
    await act(async () => runtime.closeTab(tabId))

    expect(runtime.activeTabs).toHaveLength(0)
    expect(transport.close).toHaveBeenCalledOnce()
    expect(storedMetadata().conversations.chat).toEqual({
      open: true,
      activeTabId: null,
      tabs: [],
    })
  })

  it('persists rename, selection, dock, and maximize state without runtime fields', async () => {
    const transport = new FakeTransport()
    render(<Harness transport={transport} target={ready('chat', 'D:/workspace')} />)
    await toggleDock()
    const firstId = runtime.activeTabId!
    await createTab()
    const secondId = runtime.activeTabId!

    act(() => {
      runtime.renameTab(firstId, 'Dev server')
      runtime.selectTab(firstId)
      runtime.toggleMaximized()
    })
    await toggleDock()

    expect(runtime.activeTabId).toBe(firstId)
    expect(runtime.isDockOpen).toBe(false)
    expect(runtime.isMaximized).toBe(true)
    const stored = storedMetadata()
    expect(stored.conversations.chat?.tabs.map((tab) => tab.id)).toEqual([firstId, secondId])
    expect(stored.conversations.chat?.tabs[0]?.label).toBe('Dev server')
    expect(JSON.stringify(stored)).not.toMatch(/sessionId|status|error|generation|buffer|subscriber/)
  })

  it('closes a conversation, clears subscribers, and removes metadata despite close errors', async () => {
    const transport = new FakeTransport()
    render(<Harness transport={transport} target={ready('chat', 'D:/workspace')} />)
    await toggleDock()
    await createTab()
    const listener = vi.fn()
    const firstTabId = runtime.activeTabs[0]!.tabId
    runtime.subscribeOutput(firstTabId, listener)
    transport.close.mockRejectedValueOnce(new Error('first close failed'))

    let closeError: unknown
    await act(async () => {
      try {
        await runtime.closeConversation('chat', true)
      } catch (cause) {
        closeError = cause
      }
    })
    expect(closeError).toMatchObject({ message: 'first close failed' })
    transport.creates[0]?.onEvent({ event: 'output', data: { bytes: new Uint8Array([1]) } })

    expect(runtime.allTabs).toHaveLength(0)
    expect(listener).not.toHaveBeenCalled()
    expect(storedMetadata().conversations.chat).toBeUndefined()
    expect(transport.close).toHaveBeenCalledTimes(2)
  })

  it('does not resolve closeConversation until its pending create orphan is closed', async () => {
    let resolveCreate: ((descriptor: TerminalDescriptor) => void) | undefined
    const createGate = new Promise<TerminalDescriptor>((resolve) => {
      resolveCreate = resolve
    })
    const transport = new FakeTransport()
    transport.create.mockImplementation((request, onEvent) => {
      transport.creates.push({ request, onEvent })
      return createGate
    })
    render(<Harness transport={transport} target={ready('chat', 'D:/workspace')} />)
    const opening = runtime.toggleDock()
    await waitFor(() => expect(transport.create).toHaveBeenCalledOnce())

    let cleanupSettled = false
    const cleanupPromise = runtime.closeConversation('chat', true).finally(() => {
      cleanupSettled = true
    })
    await act(async () => Promise.resolve())
    expect(cleanupSettled).toBe(false)

    resolveCreate?.({ sessionId: 'conversation-orphan', shellName: 'Shell', cwd: 'D:/workspace' })
    await act(async () => Promise.all([opening, cleanupPromise]))

    expect(transport.close).toHaveBeenCalledWith('chat', 'conversation-orphan')
    expect(runtime.allTabs).toHaveLength(0)
    expect(storedMetadata().conversations.chat).toBeUndefined()
  })

  it('closeAll removes every runtime and metadata in finally and is concurrent-idempotent', async () => {
    let rejectCloseAll: ((cause: Error) => void) | undefined
    const deferred = new Promise<void>((_resolve, reject) => {
      rejectCloseAll = reject
    })
    const transport = new FakeTransport()
    transport.closeAll.mockImplementation(() => deferred)
    render(<Harness transport={transport} target={ready('chat', 'D:/workspace')} />)
    await toggleDock()

    const first = runtime.closeAll(true)
    const second = runtime.closeAll(true)
    expect(first).toBe(second)
    await runtime.createTab()
    await runtime.toggleDock()
    const ignoredUnregister = runtime.registerConversation(ready('blocked-chat', 'D:/blocked'))
    expect(transport.create).toHaveBeenCalledOnce()
    expect(runtime.activeConversation?.conversationId).toBe('chat')
    ignoredUnregister()
    rejectCloseAll?.(new Error('close all failed'))
    await expect(first).rejects.toThrow('close all failed')
    await waitFor(() => expect(runtime.allTabs).toHaveLength(0))

    expect(transport.closeAll).toHaveBeenCalledTimes(2)
    expect(localStorage.getItem(TERMINAL_METADATA_STORAGE_KEY)).toBeNull()

    let unregister: () => void = () => undefined
    act(() => {
      unregister = runtime.registerConversation(ready('new-chat', 'D:/new'))
    })
    await toggleDock()
    expect(transport.create).toHaveBeenCalledTimes(2)
    unregister()
  })

  it('waits through both closeAll barriers when a descriptor arrives before the first response', async () => {
    let resolveCreate: ((descriptor: TerminalDescriptor) => void) | undefined
    const createGate = new Promise<TerminalDescriptor>((resolve) => {
      resolveCreate = resolve
    })
    let releaseFirstBarrier: (() => void) | undefined
    const firstBarrier = new Promise<void>((resolve) => {
      releaseFirstBarrier = resolve
    })
    const transport = new FakeTransport()
    transport.create.mockImplementation((request, onEvent) => {
      transport.creates.push({ request, onEvent })
      return createGate
    })
    transport.closeAll
      .mockImplementationOnce(() => firstBarrier)
      .mockResolvedValueOnce(undefined)
    render(<Harness transport={transport} target={ready('chat', 'D:/workspace')} />)
    const opening = runtime.toggleDock()
    await waitFor(() => expect(transport.create).toHaveBeenCalledOnce())
    const listener = vi.fn()
    runtime.subscribeOutput(runtime.activeTabId!, listener)

    let cleanupSettled = false
    const cleanupPromise = runtime.closeAll(true).finally(() => {
      cleanupSettled = true
    })
    await waitFor(() => expect(transport.closeAll).toHaveBeenCalledOnce())
    resolveCreate?.({ sessionId: 'early-orphan', shellName: 'Shell', cwd: 'D:/workspace' })
    await waitFor(() => expect(transport.close).toHaveBeenCalledWith('chat', 'early-orphan'))
    act(() => {
      transport.creates[0]?.onEvent({ event: 'output', data: { bytes: new Uint8Array([1]) } })
      transport.creates[0]?.onEvent({
        event: 'error',
        data: { code: 'late', message: 'late cleanup event' },
      })
    })

    expect(cleanupSettled).toBe(false)
    expect(listener).not.toHaveBeenCalled()
    expect(transport.closeAll).toHaveBeenCalledOnce()

    releaseFirstBarrier?.()
    await act(async () => Promise.all([opening, cleanupPromise]))

    expect(transport.closeAll).toHaveBeenCalledTimes(2)
    expect(runtime.allTabs).toHaveLength(0)
  })

  it('waits for a create not yet in the manager and still runs the final barrier after orphan failure', async () => {
    let enterManager: (() => void) | undefined
    const beforeManager = new Promise<void>((resolve) => {
      enterManager = resolve
    })
    let managerCreates = 0
    const transport = new FakeTransport()
    transport.create.mockImplementation(async (request, onEvent) => {
      transport.creates.push({ request, onEvent })
      await beforeManager
      managerCreates += 1
      return { sessionId: 'late-orphan', shellName: 'Shell', cwd: request.cwd }
    })
    transport.close.mockRejectedValueOnce(new Error('orphan close failed'))
    render(<Harness transport={transport} target={ready('chat', 'D:/workspace')} />)
    const opening = runtime.toggleDock()
    const openingResult = opening.catch((cause: unknown) => cause)
    await waitFor(() => expect(transport.create).toHaveBeenCalledOnce())

    let cleanupSettled = false
    const cleanupPromise = runtime.closeAll(true).finally(() => {
      cleanupSettled = true
    })
    await waitFor(() => expect(transport.closeAll).toHaveBeenCalledOnce())

    expect(managerCreates).toBe(0)
    expect(cleanupSettled).toBe(false)
    expect(transport.closeAll).toHaveBeenCalledOnce()

    enterManager?.()
    await expect(cleanupPromise).rejects.toThrow('orphan close failed')
    await expect(openingResult).resolves.toMatchObject({
      code: 'terminal.cleanup_failed',
    })

    expect(managerCreates).toBe(1)
    expect(transport.close).toHaveBeenCalledWith('chat', 'late-orphan')
    expect(transport.closeAll).toHaveBeenCalledTimes(2)
    await waitFor(() => expect(runtime.allTabs).toHaveLength(0))
  })

  it('returns the first closeAll failure after pending and final cleanup also fail', async () => {
    let resolveCreate: ((descriptor: TerminalDescriptor) => void) | undefined
    const createGate = new Promise<TerminalDescriptor>((resolve) => {
      resolveCreate = resolve
    })
    const transport = new FakeTransport()
    transport.create.mockImplementation((request, onEvent) => {
      transport.creates.push({ request, onEvent })
      return createGate
    })
    transport.closeAll
      .mockRejectedValueOnce(new Error('first barrier failed'))
      .mockRejectedValueOnce(new Error('final barrier failed'))
    transport.close.mockRejectedValueOnce(new Error('orphan failed'))
    render(<Harness transport={transport} target={ready('chat', 'D:/workspace')} />)
    const openingResult = runtime.toggleDock().catch((cause: unknown) => cause)
    await waitFor(() => expect(transport.create).toHaveBeenCalledOnce())

    const cleanupPromise = runtime.closeAll(true)
    await waitFor(() => expect(transport.closeAll).toHaveBeenCalledOnce())
    resolveCreate?.({ sessionId: 'orphan', shellName: 'Shell', cwd: 'D:/workspace' })

    await expect(cleanupPromise).rejects.toThrow('first barrier failed')
    await openingResult
    expect(transport.closeAll).toHaveBeenCalledTimes(2)
  })

  it('prevents a pending closeTab from recreating metadata after closeAll', async () => {
    let releaseClose: (() => void) | undefined
    const closeGate = new Promise<void>((resolve) => {
      releaseClose = resolve
    })
    const transport = new FakeTransport()
    transport.close.mockImplementation(() => closeGate)
    render(<Harness transport={transport} target={ready('chat', 'D:/workspace')} />)
    await toggleDock()

    const closingTab = runtime.closeTab(runtime.activeTabId!)
    await act(async () => runtime.closeAll(true))
    releaseClose?.()
    await act(async () => closingTab)

    expect(localStorage.getItem(TERMINAL_METADATA_STORAGE_KEY)).toBeNull()
    expect(runtime.allTabs).toHaveLength(0)
  })

  it('maps write failures to fatal error but leaves resize failures non-fatal and retryable', async () => {
    const transport = new FakeTransport()
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    render(<Harness transport={transport} target={ready('chat', 'D:/workspace')} />)
    await toggleDock()
    const tabId = runtime.activeTabId!
    transport.write.mockRejectedValueOnce(new Error('write failed'))

    let writeError: unknown
    await act(async () => {
      try {
        await runtime.write(tabId, 'pwd\r')
      } catch (cause) {
        writeError = cause
      }
    })
    expect(writeError).toMatchObject({ code: 'terminal.write_failed' })
    expect(runtime.activeTabs[0]).toMatchObject({ status: 'error' })

    await act(async () => runtime.restartTab(tabId))
    transport.resize.mockRejectedValueOnce(new Error('resize failed'))
    await expect(runtime.resize(tabId, 100, 30)).resolves.toBeUndefined()
    expect(runtime.activeTabs[0]).toMatchObject({ status: 'running', error: null })
    await expect(runtime.resize(tabId, 101, 31)).resolves.toBeUndefined()
    expect(transport.resize).toHaveBeenCalledTimes(2)
    expect(warn).toHaveBeenCalledWith('Terminal resize failed', 'terminal.resize_failed')
  })

  it('tracks the newest registration and cleanup never closes sessions', async () => {
    const transport = new FakeTransport()
    const registrations: Array<() => void> = []
    function ManualRegistrations() {
      const { registerConversation } = useTerminalRuntime()
      useEffect(() => {
        registrations.push(registerConversation(ready('chat-a', 'D:/a')))
        registrations.push(registerConversation(ready('chat-b', 'D:/b')))
      }, [registerConversation])
      return null
    }
    render(
      <TerminalRuntimeProvider transport={transport}>
        <Probe />
        <ManualRegistrations />
      </TerminalRuntimeProvider>,
    )
    await waitFor(() => expect(runtime.activeConversation?.conversationId).toBe('chat-b'))

    act(() => registrations[0]?.())
    expect(runtime.activeConversation?.conversationId).toBe('chat-b')
    act(() => registrations[1]?.())

    expect(runtime.activeConversation).toBeNull()
    expect(transport.close).not.toHaveBeenCalled()
  })

  it('deduplicates restoration across rapid ready registrations', async () => {
    seedMetadata({
      height: 0,
      conversations: {
        chat: {
          open: true,
          activeTabId: 'saved-tab',
          tabs: [{ id: 'saved-tab', label: 'Shell', launchDirectory: 'D:/workspace' }],
        },
      },
    })
    const transport = new FakeTransport()
    function DoubleRegistration() {
      const { registerConversation } = useTerminalRuntime()
      useEffect(() => {
        const first = registerConversation(ready('chat', 'D:/workspace'))
        const second = registerConversation(ready('chat', 'D:/workspace'))
        return () => {
          first()
          second()
        }
      }, [registerConversation])
      return null
    }

    render(
      <TerminalRuntimeProvider transport={transport}>
        <Probe />
        <DoubleRegistration />
      </TerminalRuntimeProvider>,
    )
    await waitFor(() => expect(runtime.allTabs[0]?.status).toBe('running'))

    expect(transport.create).toHaveBeenCalledOnce()
  })

  it('restores duplicate persisted tab ids only for the first conversation', async () => {
    seedMetadata({
      height: 0,
      conversations: {
        'chat-a': {
          open: true,
          activeTabId: 'shared-tab',
          tabs: [
            { id: 'shared-tab', label: 'First owner', launchDirectory: 'D:/a' },
          ],
        },
        'chat-b': {
          open: true,
          activeTabId: 'shared-tab',
          tabs: [
            { id: 'shared-tab', label: 'Duplicate owner', launchDirectory: 'D:/b' },
            { id: 'chat-b-tab', label: 'Second shell', launchDirectory: 'D:/b' },
          ],
        },
      },
    })
    const transport = new FakeTransport()
    const { rerender } = render(
      <Harness transport={transport} target={ready('chat-a', 'D:/a')} />,
    )
    await waitFor(() => expect(runtime.activeTabs[0]?.status).toBe('running'))

    rerender(<Harness transport={transport} target={ready('chat-b', 'D:/b')} />)
    await waitFor(() => expect(runtime.activeTabs[0]?.status).toBe('running'))

    expect(transport.create).toHaveBeenCalledTimes(2)
    expect(runtime.allTabs.map((tab) => [tab.tabId, tab.conversationId])).toEqual([
      ['shared-tab', 'chat-a'],
      ['chat-b-tab', 'chat-b'],
    ])
    expect(runtime.activeTabId).toBe('chat-b-tab')
    expect(storedMetadata().conversations['chat-b']).toMatchObject({
      activeTabId: 'chat-b-tab',
      tabs: [{ id: 'chat-b-tab' }],
    })
  })

  it('deduplicates restored creation under StrictMode and concurrent dock toggles', async () => {
    seedMetadata({
      height: 0,
      conversations: {
        chat: {
          open: true,
          activeTabId: 'saved-tab',
          tabs: [{ id: 'saved-tab', label: 'Shell', launchDirectory: 'D:/workspace' }],
        },
      },
    })
    const transport = new FakeTransport()
    render(
      <StrictMode>
        <Harness transport={transport} target={ready('chat', 'D:/workspace')} />
      </StrictMode>,
    )
    await waitFor(() => expect(runtime.allTabs[0]?.status).toBe('running'))

    await act(async () => Promise.all([runtime.toggleDock(), runtime.toggleDock()]))

    expect(transport.create).toHaveBeenCalledOnce()
    expect(transport.close).not.toHaveBeenCalled()
  })
})
