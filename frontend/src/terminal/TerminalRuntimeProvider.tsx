import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useRef,
  useState,
  type PropsWithChildren,
} from 'react'

import { looksAbsolute } from '@/lib/folderPicker'
import { isDesktopRuntime } from '@/lib/runtime'
import {
  clearTerminalMetadata,
  loadTerminalMetadata,
  saveTerminalMetadata,
  type TerminalConversationMetadata,
  type TerminalMetadataStore,
  type TerminalTabMetadata,
} from '@/terminal/metadataStore'
import { createTauriTerminalTransport } from '@/terminal/tauriTransport'
import {
  createUnavailableTerminalTransport,
  normalizeTerminalTransportError,
  TerminalTransportError,
  type TerminalTransport,
} from '@/terminal/transport'
import type { TerminalConversationTarget, TerminalEvent } from '@/terminal/types'

const DEFAULT_COLS = 80
const DEFAULT_ROWS = 24
const MAX_BUFFERED_OUTPUT_BYTES = 256 * 1024

export type TerminalOutputListener = (bytes: Uint8Array) => void

export interface TerminalRuntimeTab {
  tabId: string
  conversationId: string
  sessionId: string | null
  label: string
  launchDirectory: string
  status: 'starting' | 'running' | 'exited' | 'error'
  exitCode: number | null
  error: TerminalTransportError | null
}

export interface TerminalRuntimeContextValue {
  activeConversation: TerminalConversationTarget | null
  allTabs: TerminalRuntimeTab[]
  activeTabs: TerminalRuntimeTab[]
  activeTabId: string | null
  isDockOpen: boolean
  isMaximized: boolean
  registerConversation(target: TerminalConversationTarget): () => void
  toggleDock(): Promise<void>
  createTab(): Promise<void>
  selectTab(tabId: string): void
  renameTab(tabId: string, label: string): void
  closeTab(tabId: string): Promise<void>
  restartTab(tabId: string): Promise<void>
  closeConversation(conversationId: string, clearMetadata: boolean): Promise<void>
  closeAll(clearMetadata: boolean): Promise<void>
  toggleMaximized(): void
  subscribeOutput(tabId: string, listener: TerminalOutputListener): () => void
  write(tabId: string, data: string): Promise<void>
  resize(tabId: string, cols: number, rows: number): Promise<void>
}

interface OutputChannel {
  listeners: Set<TerminalOutputListener>
  buffering: boolean
  bufferedChunks: Uint8Array[]
  bufferedBytes: number
}

interface RuntimeEntry extends TerminalRuntimeTab {
  generation: number
  fallbackCwd: string
  output: OutputChannel
}

interface Registration {
  id: number
  target: TerminalConversationTarget
}

export interface TerminalRuntimeProviderProps extends PropsWithChildren {
  transport?: TerminalTransport
}

const TerminalRuntimeContext = createContext<TerminalRuntimeContextValue | null>(null)

function createOutputChannel(): OutputChannel {
  return {
    listeners: new Set(),
    buffering: true,
    bufferedChunks: [],
    bufferedBytes: 0,
  }
}

function clearOutputChannel(channel: OutputChannel): void {
  channel.listeners.clear()
  channel.bufferedChunks = []
  channel.bufferedBytes = 0
  channel.buffering = false
}

function notifyListener(listener: TerminalOutputListener, bytes: Uint8Array): void {
  try {
    listener(bytes)
  } catch (cause) {
    console.error('Terminal output listener failed', cause)
  }
}

function dispatchOutput(channel: OutputChannel, bytes: Uint8Array): void {
  if (channel.listeners.size > 0 || !channel.buffering) {
    for (const listener of channel.listeners) notifyListener(listener, bytes)
    return
  }

  let chunk = bytes.slice()
  if (chunk.byteLength >= MAX_BUFFERED_OUTPUT_BYTES) {
    chunk = chunk.slice(chunk.byteLength - MAX_BUFFERED_OUTPUT_BYTES)
    channel.bufferedChunks = [chunk]
    channel.bufferedBytes = chunk.byteLength
    return
  }

  channel.bufferedChunks.push(chunk)
  channel.bufferedBytes += chunk.byteLength
  let overflow = channel.bufferedBytes - MAX_BUFFERED_OUTPUT_BYTES
  while (overflow > 0) {
    const first = channel.bufferedChunks[0]
    if (first === undefined) break
    if (first.byteLength <= overflow) {
      channel.bufferedChunks.shift()
      channel.bufferedBytes -= first.byteLength
      overflow -= first.byteLength
      continue
    }
    channel.bufferedChunks[0] = first.slice(overflow)
    channel.bufferedBytes -= overflow
    overflow = 0
  }
}

function cloneMetadata(metadata: TerminalMetadataStore): TerminalMetadataStore {
  return {
    height: metadata.height,
    conversations: { ...metadata.conversations },
  }
}

function cloneConversation(
  conversation: TerminalConversationMetadata | undefined,
): TerminalConversationMetadata {
  return conversation === undefined
    ? { open: false, activeTabId: null, tabs: [] }
    : { ...conversation, tabs: conversation.tabs.map((tab) => ({ ...tab })) }
}

function createTabId(): string {
  if (typeof crypto.randomUUID === 'function') return crypto.randomUUID()
  return `terminal-${Date.now()}-${Math.random().toString(36).slice(2)}`
}

function publicTab(entry: RuntimeEntry): TerminalRuntimeTab {
  return {
    tabId: entry.tabId,
    conversationId: entry.conversationId,
    sessionId: entry.sessionId,
    label: entry.label,
    launchDirectory: entry.launchDirectory,
    status: entry.status,
    exitCode: entry.exitCode,
    error: entry.error,
  }
}

function firstFailure(results: PromiseSettledResult<void>[]): unknown | undefined {
  const failed = results.find(
    (result): result is PromiseRejectedResult => result.status === 'rejected',
  )
  return failed?.reason
}

export function TerminalRuntimeProvider({
  children,
  transport: injectedTransport,
}: TerminalRuntimeProviderProps) {
  const transportRef = useRef<TerminalTransport | null>(null)
  if (transportRef.current === null) {
    transportRef.current = injectedTransport ?? (
      isDesktopRuntime()
        ? createTauriTerminalTransport()
        : createUnavailableTerminalTransport()
    )
  }
  const transport = transportRef.current

  const initialMetadataRef = useRef<TerminalMetadataStore | null>(null)
  if (initialMetadataRef.current === null) {
    initialMetadataRef.current = loadTerminalMetadata()
  }
  const metadataRef = useRef(initialMetadataRef.current)
  const [metadata, setMetadata] = useState(initialMetadataRef.current)
  const runtimeRef = useRef(new Map<string, RuntimeEntry>())
  const generationsRef = useRef(new Map<string, number>())
  const restoredConversationsRef = useRef(new Set<string>())
  const readyCwdsRef = useRef(new Map<string, string>())
  const closingTabsRef = useRef(new Map<string, Promise<void>>())
  const restartingTabsRef = useRef(new Map<string, Promise<void>>())
  const restartTokensRef = useRef(new Map<string, symbol>())
  const pendingStartsRef = useRef(new Set<Promise<void>>())
  const pendingStartsByTabRef = useRef(new Map<string, Set<Promise<void>>>())
  const closeAllRef = useRef<Promise<void> | null>(null)
  const isClosingAllRef = useRef(false)
  const metadataEpochRef = useRef(0)
  const registrationsRef = useRef<Registration[]>([])
  const nextRegistrationIdRef = useRef(0)
  const [activeConversation, setActiveConversation] = useState<TerminalConversationTarget | null>(null)
  const activeConversationRef = useRef<TerminalConversationTarget | null>(null)
  const [runtimeVersion, setRuntimeVersion] = useState(0)
  const [isMaximized, setIsMaximized] = useState(false)

  const publishRuntime = useCallback(() => {
    setRuntimeVersion((version) => version + 1)
  }, [])

  const commitMetadata = useCallback((mutate: (draft: TerminalMetadataStore) => void) => {
    const draft = cloneMetadata(metadataRef.current)
    mutate(draft)
    metadataRef.current = draft
    saveTerminalMetadata(draft)
    setMetadata(draft)
  }, [])

  const updateConversationMetadata = useCallback((
    conversationId: string,
    mutate: (conversation: TerminalConversationMetadata) => void,
  ) => {
    commitMetadata((draft) => {
      const conversation = cloneConversation(draft.conversations[conversationId])
      mutate(conversation)
      draft.conversations[conversationId] = conversation
    })
  }, [commitMetadata])

  const updateTabMetadata = useCallback((
    conversationId: string,
    tabId: string,
    mutate: (tab: TerminalTabMetadata) => void,
  ) => {
    updateConversationMetadata(conversationId, (conversation) => {
      const tab = conversation.tabs.find((candidate) => candidate.id === tabId)
      if (tab !== undefined) mutate(tab)
    })
  }, [updateConversationMetadata])

  const nextGeneration = useCallback((tabId: string): number => {
    const generation = (generationsRef.current.get(tabId) ?? 0) + 1
    generationsRef.current.set(tabId, generation)
    return generation
  }, [])

  const invalidateGeneration = useCallback((tabId: string): void => {
    const generation = (generationsRef.current.get(tabId) ?? 0) + 1
    generationsRef.current.set(tabId, generation)
    const entry = runtimeRef.current.get(tabId)
    if (entry !== undefined) entry.generation = generation
  }, [])

  const trackStart = useCallback((tabId: string, operation: Promise<void>): Promise<void> => {
    pendingStartsRef.current.add(operation)
    const tabOperations = pendingStartsByTabRef.current.get(tabId) ?? new Set<Promise<void>>()
    tabOperations.add(operation)
    pendingStartsByTabRef.current.set(tabId, tabOperations)
    void operation.finally(() => {
      pendingStartsRef.current.delete(operation)
      const currentTabOperations = pendingStartsByTabRef.current.get(tabId)
      currentTabOperations?.delete(operation)
      if (currentTabOperations?.size === 0) pendingStartsByTabRef.current.delete(tabId)
    }).catch(() => undefined)
    return operation
  }, [])

  const settlePendingStarts = useCallback(async (
    operationsForSnapshot: () => Promise<void>[],
  ): Promise<unknown | undefined> => {
    let failure: unknown | undefined
    while (true) {
      const snapshot = operationsForSnapshot()
      if (snapshot.length === 0) return failure
      const results = await Promise.allSettled(snapshot)
      failure ??= firstFailure(results)
    }
  }, [])

  const settlePendingStartsForTab = useCallback((tabId: string) => (
    settlePendingStarts(() => Array.from(pendingStartsByTabRef.current.get(tabId) ?? []))
  ), [settlePendingStarts])

  const settleAllPendingStarts = useCallback(() => (
    settlePendingStarts(() => Array.from(pendingStartsRef.current))
  ), [settlePendingStarts])

  const startRuntimeTab = useCallback((
    tab: TerminalTabMetadata,
    conversationId: string,
    fallbackCwd: string,
    useDescriptorLabel: boolean,
    output = runtimeRef.current.get(tab.id)?.output ?? createOutputChannel(),
    requestedCwd = looksAbsolute(tab.launchDirectory) ? tab.launchDirectory : fallbackCwd,
    mayFallback = looksAbsolute(tab.launchDirectory) && tab.launchDirectory !== fallbackCwd,
  ): Promise<void> => {
    if (isClosingAllRef.current) return Promise.resolve()
    const generation = nextGeneration(tab.id)
    const entry: RuntimeEntry = {
      tabId: tab.id,
      conversationId,
      sessionId: null,
      label: tab.label,
      launchDirectory: requestedCwd,
      status: 'starting',
      exitCode: null,
      error: null,
      generation,
      fallbackCwd,
      output,
    }
    runtimeRef.current.set(tab.id, entry)
    if (requestedCwd !== tab.launchDirectory) {
      updateTabMetadata(conversationId, tab.id, (savedTab) => {
        savedTab.launchDirectory = requestedCwd
      })
    }
    publishRuntime()

    const onEvent = (event: TerminalEvent) => {
      if (isClosingAllRef.current) return
      const current = runtimeRef.current.get(tab.id)
      if (current === undefined || current.generation !== generation) return
      if (event.event === 'output') {
        dispatchOutput(current.output, event.data.bytes)
        return
      }
      if (event.event === 'exit') {
        current.status = 'exited'
        current.exitCode = event.data.code
        current.error = null
      } else {
        current.status = 'error'
        current.exitCode = null
        current.error = new TerminalTransportError(event.data.code, event.data.message)
      }
      publishRuntime()
    }

    const operation = Promise.resolve().then(async () => {
      let current = runtimeRef.current.get(tab.id)
      if (
        isClosingAllRef.current ||
        current === undefined ||
        current.generation !== generation
      ) {
        return
      }

      let descriptor
      try {
        descriptor = await transport.create(
          {
            conversationId,
            cwd: requestedCwd,
            cols: DEFAULT_COLS,
            rows: DEFAULT_ROWS,
          },
          onEvent,
        )
      } catch (cause) {
        current = runtimeRef.current.get(tab.id)
        if (
          isClosingAllRef.current ||
          current === undefined ||
          current.generation !== generation
        ) {
          return
        }
        if (mayFallback && requestedCwd !== fallbackCwd) {
          current.output.bufferedChunks = []
          current.output.bufferedBytes = 0
          updateTabMetadata(conversationId, tab.id, (savedTab) => {
            savedTab.launchDirectory = fallbackCwd
          })
          await startRuntimeTab(
            { ...tab, launchDirectory: fallbackCwd },
            conversationId,
            fallbackCwd,
            useDescriptorLabel,
            output,
            fallbackCwd,
            false,
          )
          return
        }
        current.status = 'error'
        current.error = normalizeTerminalTransportError(
          cause,
          'terminal.spawn_failed',
          'Unable to start the terminal',
        )
        current.exitCode = null
        publishRuntime()
        return
      }

      current = runtimeRef.current.get(tab.id)
      if (
        isClosingAllRef.current ||
        current === undefined ||
        current.generation !== generation
      ) {
        try {
          await transport.close(conversationId, descriptor.sessionId)
        } catch (cause) {
          throw normalizeTerminalTransportError(cause, 'terminal.cleanup_failed')
        }
        return
      }
      current.sessionId = descriptor.sessionId
      current.launchDirectory = descriptor.cwd
      if (useDescriptorLabel) current.label = descriptor.shellName
      if (current.status === 'starting') current.status = 'running'
      updateTabMetadata(conversationId, tab.id, (savedTab) => {
        savedTab.launchDirectory = descriptor.cwd
        if (useDescriptorLabel) savedTab.label = descriptor.shellName
      })
      publishRuntime()
    })
    return trackStart(tab.id, operation)
  }, [nextGeneration, publishRuntime, trackStart, transport, updateTabMetadata])

  const ensureConversationRestored = useCallback((
    conversationId: string,
    cwd: string,
  ): void => {
    if (isClosingAllRef.current) return
    if (restoredConversationsRef.current.has(conversationId)) return
    restoredConversationsRef.current.add(conversationId)
    const conversation = metadataRef.current.conversations[conversationId]
    if (conversation === undefined) return
    if (
      conversation.tabs.length > 0 &&
      !conversation.tabs.some((tab) => tab.id === conversation.activeTabId)
    ) {
      updateConversationMetadata(conversationId, (savedConversation) => {
        savedConversation.activeTabId = savedConversation.tabs[0]?.id ?? null
      })
    }
    for (const tab of conversation.tabs) {
      if (!runtimeRef.current.has(tab.id)) {
        void startRuntimeTab(tab, conversationId, cwd, false).catch(() => undefined)
      }
    }
  }, [startRuntimeTab, updateConversationMetadata])

  const createTabFor = useCallback(async (
    conversationId: string,
    cwd: string,
  ): Promise<void> => {
    if (isClosingAllRef.current) return
    const tab: TerminalTabMetadata = {
      id: createTabId(),
      label: 'Terminal',
      launchDirectory: cwd,
    }
    updateConversationMetadata(conversationId, (conversation) => {
      conversation.tabs.push(tab)
      conversation.activeTabId = tab.id
      conversation.open = true
    })
    await startRuntimeTab(tab, conversationId, cwd, true)
  }, [startRuntimeTab, updateConversationMetadata])

  const registerConversation = useCallback((target: TerminalConversationTarget) => {
    if (isClosingAllRef.current) return () => undefined
    const registration: Registration = {
      id: ++nextRegistrationIdRef.current,
      target,
    }
    registrationsRef.current.push(registration)
    activeConversationRef.current = target
    setActiveConversation(target)
    if (target.availability === 'ready' && !isClosingAllRef.current) {
      readyCwdsRef.current.set(target.conversationId, target.cwd)
      for (const entry of runtimeRef.current.values()) {
        if (entry.conversationId === target.conversationId) entry.fallbackCwd = target.cwd
      }
      ensureConversationRestored(target.conversationId, target.cwd)
    }

    let registered = true
    return () => {
      if (!registered) return
      registered = false
      registrationsRef.current = registrationsRef.current.filter(
        (candidate) => candidate.id !== registration.id,
      )
      const next = registrationsRef.current.at(-1)?.target ?? null
      activeConversationRef.current = next
      setActiveConversation(next)
    }
  }, [ensureConversationRestored])

  const createTab = useCallback(async (): Promise<void> => {
    if (isClosingAllRef.current) return
    const target = activeConversationRef.current
    if (target?.availability !== 'ready') return
    await createTabFor(target.conversationId, target.cwd)
  }, [createTabFor])

  const toggleDock = useCallback(async (): Promise<void> => {
    if (isClosingAllRef.current) return
    const target = activeConversationRef.current
    if (target === null) return
    const current = metadataRef.current.conversations[target.conversationId]
    const opening = !(current?.open ?? false)
    updateConversationMetadata(target.conversationId, (conversation) => {
      conversation.open = opening
    })
    if (
      opening &&
      target.availability === 'ready' &&
      (current?.tabs.length ?? 0) === 0
    ) {
      await createTabFor(target.conversationId, target.cwd)
    }
  }, [createTabFor, updateConversationMetadata])

  const selectTab = useCallback((tabId: string): void => {
    if (isClosingAllRef.current) return
    const entry = runtimeRef.current.get(tabId)
    if (entry === undefined) return
    updateConversationMetadata(entry.conversationId, (conversation) => {
      if (conversation.tabs.some((tab) => tab.id === tabId)) {
        conversation.activeTabId = tabId
      }
    })
  }, [updateConversationMetadata])

  const renameTab = useCallback((tabId: string, label: string): void => {
    if (isClosingAllRef.current) return
    const entry = runtimeRef.current.get(tabId)
    if (entry === undefined) return
    entry.label = label
    updateTabMetadata(entry.conversationId, tabId, (tab) => {
      tab.label = label
    })
    publishRuntime()
  }, [publishRuntime, updateTabMetadata])

  const closeTabRuntime = useCallback((
    tabId: string,
    removeMetadata: boolean,
  ): Promise<void> => {
    restartTokensRef.current.delete(tabId)
    const existing = closingTabsRef.current.get(tabId)
    if (existing !== undefined) return existing
    const entry = runtimeRef.current.get(tabId)
    invalidateGeneration(tabId)
    const operationMetadataEpoch = metadataEpochRef.current
    const metadataConversationId = entry?.conversationId ?? Object.entries(
      metadataRef.current.conversations,
    ).find(([, conversation]) => conversation.tabs.some((tab) => tab.id === tabId))?.[0]
    const operation = Promise.resolve().then(async () => {
      let failure = await settlePendingStartsForTab(tabId)
      try {
        if (entry?.sessionId !== null && entry?.sessionId !== undefined) {
          await transport.close(entry.conversationId, entry.sessionId)
        }
      } catch (cause) {
        failure ??= normalizeTerminalTransportError(cause, 'terminal.cleanup_failed')
      } finally {
        if (entry !== undefined) {
          if (runtimeRef.current.get(tabId) === entry) runtimeRef.current.delete(tabId)
          clearOutputChannel(entry.output)
        }
        if (
          removeMetadata &&
          metadataEpochRef.current === operationMetadataEpoch &&
          metadataConversationId !== undefined
        ) {
          updateConversationMetadata(metadataConversationId, (conversation) => {
            const index = conversation.tabs.findIndex((tab) => tab.id === tabId)
            if (index < 0) return
            conversation.tabs.splice(index, 1)
            if (conversation.activeTabId === tabId) {
              conversation.activeTabId = conversation.tabs[index]?.id
                ?? conversation.tabs[index - 1]?.id
                ?? null
            }
          })
        }
        publishRuntime()
      }
      if (failure !== undefined) throw failure
    })
    closingTabsRef.current.set(tabId, operation)
    void operation.finally(() => {
      if (closingTabsRef.current.get(tabId) === operation) {
        closingTabsRef.current.delete(tabId)
      }
    }).catch(() => undefined)
    return operation
  }, [
    invalidateGeneration,
    publishRuntime,
    settlePendingStartsForTab,
    transport,
    updateConversationMetadata,
  ])

  const closeTab = useCallback((tabId: string) => closeTabRuntime(tabId, true), [closeTabRuntime])

  const restartTab = useCallback((tabId: string): Promise<void> => {
    if (isClosingAllRef.current) return Promise.resolve()
    const existing = restartingTabsRef.current.get(tabId)
    if (existing !== undefined) return existing
    const entry = runtimeRef.current.get(tabId)
    if (entry === undefined) return Promise.resolve()
    const conversation = metadataRef.current.conversations[entry.conversationId]
    const tab = conversation?.tabs.find((candidate) => candidate.id === tabId)
    if (tab === undefined) return Promise.resolve()

    invalidateGeneration(tabId)
    const restartToken = Symbol(tabId)
    restartTokensRef.current.set(tabId, restartToken)
    const operation = (async () => {
      try {
        if (entry.sessionId !== null) {
          await transport.close(entry.conversationId, entry.sessionId)
        }
      } catch (cause) {
        const error = normalizeTerminalTransportError(cause, 'terminal.cleanup_failed')
        if (
          restartTokensRef.current.get(tabId) === restartToken &&
          runtimeRef.current.get(tabId) === entry
        ) {
          entry.status = 'error'
          entry.exitCode = null
          entry.error = error
          publishRuntime()
        }
        throw error
      }
      const savedTabStillExists = metadataRef.current.conversations[entry.conversationId]
        ?.tabs.some((candidate) => candidate.id === tabId) ?? false
      if (
        isClosingAllRef.current ||
        restartTokensRef.current.get(tabId) !== restartToken ||
        runtimeRef.current.get(tabId) !== entry ||
        !savedTabStillExists
      ) {
        return
      }
      await startRuntimeTab(
        tab,
        entry.conversationId,
        readyCwdsRef.current.get(entry.conversationId) ?? entry.fallbackCwd,
        false,
        entry.output,
      )
    })()
    restartingTabsRef.current.set(tabId, operation)
    void operation.finally(() => {
      if (restartingTabsRef.current.get(tabId) === operation) {
        restartingTabsRef.current.delete(tabId)
      }
      if (restartTokensRef.current.get(tabId) === restartToken) {
        restartTokensRef.current.delete(tabId)
      }
    }).catch(() => undefined)
    return operation
  }, [invalidateGeneration, publishRuntime, startRuntimeTab, transport])

  const closeConversation = useCallback(async (
    conversationId: string,
    shouldClearMetadata: boolean,
  ): Promise<void> => {
    const tabIds = new Set(
      metadataRef.current.conversations[conversationId]?.tabs.map((tab) => tab.id) ?? [],
    )
    for (const entry of runtimeRef.current.values()) {
      if (entry.conversationId === conversationId) tabIds.add(entry.tabId)
    }
    const results = await Promise.allSettled(
      Array.from(tabIds, (tabId) => closeTabRuntime(tabId, false)),
    )
    restoredConversationsRef.current.delete(conversationId)
    readyCwdsRef.current.delete(conversationId)
    if (shouldClearMetadata) {
      commitMetadata((draft) => {
        delete draft.conversations[conversationId]
      })
    }
    const failure = firstFailure(results)
    if (failure !== undefined) throw failure
  }, [closeTabRuntime, commitMetadata])

  const closeAll = useCallback((shouldClearMetadata: boolean): Promise<void> => {
    if (closeAllRef.current !== null) return closeAllRef.current
    isClosingAllRef.current = true
    metadataEpochRef.current += 1
    const entries = Array.from(runtimeRef.current.values())
    restartTokensRef.current.clear()
    for (const entry of entries) invalidateGeneration(entry.tabId)
    const operation = Promise.resolve().then(async () => {
      let failure: unknown | undefined
      try {
        try {
          await transport.closeAll()
        } catch (cause) {
          failure ??= normalizeTerminalTransportError(cause, 'terminal.cleanup_failed')
        }

        try {
          const pendingFailure = await settleAllPendingStarts()
          failure ??= pendingFailure
        } catch (cause) {
          failure ??= normalizeTerminalTransportError(cause, 'terminal.cleanup_failed')
        }

        try {
          await transport.closeAll()
        } catch (cause) {
          failure ??= normalizeTerminalTransportError(cause, 'terminal.cleanup_failed')
        }
      } finally {
        for (const entry of runtimeRef.current.values()) clearOutputChannel(entry.output)
        runtimeRef.current.clear()
        pendingStartsRef.current.clear()
        pendingStartsByTabRef.current.clear()
        restoredConversationsRef.current.clear()
        readyCwdsRef.current.clear()
        if (shouldClearMetadata) {
          clearTerminalMetadata()
          const cleared: TerminalMetadataStore = { height: 0, conversations: {} }
          metadataRef.current = cleared
          setMetadata(cleared)
        }
        publishRuntime()
      }
      if (failure !== undefined) throw failure
    })
    closeAllRef.current = operation
    void operation.finally(() => {
      if (closeAllRef.current === operation) {
        closeAllRef.current = null
        isClosingAllRef.current = false
      }
    }).catch(() => undefined)
    return operation
  }, [invalidateGeneration, publishRuntime, settleAllPendingStarts, transport])

  const subscribeOutput = useCallback((
    tabId: string,
    listener: TerminalOutputListener,
  ): (() => void) => {
    const entry = runtimeRef.current.get(tabId)
    if (entry === undefined) return () => undefined
    const channel = entry.output
    channel.listeners.add(listener)
    if (channel.buffering) {
      channel.buffering = false
      const buffered = channel.bufferedChunks
      channel.bufferedChunks = []
      channel.bufferedBytes = 0
      for (const chunk of buffered) notifyListener(listener, chunk)
    }
    let subscribed = true
    return () => {
      if (!subscribed) return
      subscribed = false
      channel.listeners.delete(listener)
    }
  }, [])

  const write = useCallback(async (tabId: string, data: string): Promise<void> => {
    const entry = runtimeRef.current.get(tabId)
    if (entry?.sessionId === null || entry === undefined) {
      const error = new TerminalTransportError(
        'terminal.session_unavailable',
        'Terminal session is not ready',
      )
      if (entry !== undefined) {
        entry.status = 'error'
        entry.error = error
        publishRuntime()
      }
      throw error
    }
    const generation = entry.generation
    try {
      await transport.write(entry.conversationId, entry.sessionId, data)
    } catch (cause) {
      const error = normalizeTerminalTransportError(
        cause,
        'terminal.write_failed',
        'Unable to write to the terminal',
      )
      const current = runtimeRef.current.get(tabId)
      if (current?.generation === generation) {
        current.status = 'error'
        current.error = error
        publishRuntime()
      }
      throw error
    }
  }, [publishRuntime, transport])

  const resize = useCallback(async (
    tabId: string,
    cols: number,
    rows: number,
  ): Promise<void> => {
    const entry = runtimeRef.current.get(tabId)
    if (entry?.sessionId === null || entry === undefined) return
    try {
      await transport.resize(entry.conversationId, entry.sessionId, cols, rows)
    } catch (cause) {
      const error = normalizeTerminalTransportError(
        cause,
        'terminal.resize_failed',
        'Unable to resize the terminal',
      )
      console.warn('Terminal resize failed', error.code)
    }
  }, [transport])

  const toggleMaximized = useCallback(() => {
    setIsMaximized((value) => !value)
  }, [])

  const value = useMemo<TerminalRuntimeContextValue>(() => {
    void runtimeVersion
    const allTabs = Array.from(runtimeRef.current.values(), publicTab)
    const activeConversationId = activeConversation?.conversationId
    const activeTabs = activeConversationId === undefined
      ? []
      : allTabs.filter((tab) => tab.conversationId === activeConversationId)
    const activeMetadata = activeConversationId === undefined
      ? undefined
      : metadata.conversations[activeConversationId]
    return {
      activeConversation,
      allTabs,
      activeTabs,
      activeTabId: activeMetadata?.activeTabId ?? activeTabs[0]?.tabId ?? null,
      isDockOpen: activeMetadata?.open ?? false,
      isMaximized,
      registerConversation,
      toggleDock,
      createTab,
      selectTab,
      renameTab,
      closeTab,
      restartTab,
      closeConversation,
      closeAll,
      toggleMaximized,
      subscribeOutput,
      write,
      resize,
    }
  }, [
    activeConversation,
    closeAll,
    closeConversation,
    closeTab,
    createTab,
    isMaximized,
    metadata,
    registerConversation,
    renameTab,
    resize,
    restartTab,
    runtimeVersion,
    selectTab,
    subscribeOutput,
    toggleDock,
    toggleMaximized,
    write,
  ])

  return (
    <TerminalRuntimeContext.Provider value={value}>
      {children}
    </TerminalRuntimeContext.Provider>
  )
}

// Hooks intentionally share this module with their provider as one public contract.
// eslint-disable-next-line react-refresh/only-export-components
export function useTerminalRuntime(): TerminalRuntimeContextValue {
  const context = useContext(TerminalRuntimeContext)
  if (context === null) {
    throw new Error('useTerminalRuntime must be used within TerminalRuntimeProvider')
  }
  return context
}
