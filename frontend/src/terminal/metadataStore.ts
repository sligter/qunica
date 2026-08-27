export const TERMINAL_METADATA_STORAGE_KEY = 'qunica:terminal-metadata:v1'

export interface TerminalTabMetadata {
  id: string
  label: string
  launchDirectory: string
}

export interface TerminalConversationMetadata {
  open: boolean
  activeTabId: string | null
  tabs: TerminalTabMetadata[]
}

export interface TerminalMetadataStore {
  height: number
  conversations: Record<string, TerminalConversationMetadata>
}

export type TerminalMetadata = TerminalMetadataStore

const EMPTY_METADATA: TerminalMetadataStore = { height: 0, conversations: {} }
const DANGEROUS_CONVERSATION_KEYS = new Set([
  '__proto__',
  'constructor',
  'prototype',
])

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function getOwnProperty(
  value: Record<string, unknown>,
  key: string,
): unknown {
  return Object.prototype.hasOwnProperty.call(value, key) ? value[key] : undefined
}

function createConversationRecord(): Record<string, TerminalConversationMetadata> {
  return Object.create(null) as Record<string, TerminalConversationMetadata>
}

function createEmptyMetadata(): TerminalMetadataStore {
  return { ...EMPTY_METADATA, conversations: createConversationRecord() }
}

function sanitizeTab(value: unknown): TerminalTabMetadata | null {
  if (!isRecord(value)) return null
  const id = getOwnProperty(value, 'id')
  const label = getOwnProperty(value, 'label')
  const launchDirectory = getOwnProperty(value, 'launchDirectory')
  if (
    typeof id !== 'string' ||
    typeof label !== 'string' ||
    typeof launchDirectory !== 'string'
  ) {
    return null
  }
  return {
    id,
    label,
    launchDirectory,
  }
}

function sanitizeConversation(
  value: unknown,
  seenTabIds: Set<string>,
): TerminalConversationMetadata | null {
  if (!isRecord(value)) return null
  const rawTabs = getOwnProperty(value, 'tabs')
  const tabs = Array.isArray(rawTabs)
    ? rawTabs.flatMap((tab) => {
        const sanitized = sanitizeTab(tab)
        if (sanitized === null || seenTabIds.has(sanitized.id)) return []
        seenTabIds.add(sanitized.id)
        return [sanitized]
      })
    : []
  const open = getOwnProperty(value, 'open')
  const activeTabId = getOwnProperty(value, 'activeTabId')
  const validTabIds = new Set(tabs.map((tab) => tab.id))
  return {
    open: typeof open === 'boolean' ? open : false,
    activeTabId:
      typeof activeTabId === 'string' && validTabIds.has(activeTabId)
        ? activeTabId
        : null,
    tabs,
  }
}

function sanitizeMetadata(value: unknown): TerminalMetadataStore {
  if (!isRecord(value)) return createEmptyMetadata()

  const conversations = createConversationRecord()
  const seenTabIds = new Set<string>()
  const rawConversations = getOwnProperty(value, 'conversations')
  if (isRecord(rawConversations)) {
    for (const [conversationId, conversation] of Object.entries(rawConversations)) {
      if (DANGEROUS_CONVERSATION_KEYS.has(conversationId)) continue
      const sanitized = sanitizeConversation(conversation, seenTabIds)
      if (sanitized !== null) conversations[conversationId] = sanitized
    }
  }

  const height = getOwnProperty(value, 'height')

  return {
    height: typeof height === 'number' && Number.isFinite(height) ? height : 0,
    conversations,
  }
}

export function loadTerminalMetadata(): TerminalMetadataStore {
  try {
    const raw = localStorage.getItem(TERMINAL_METADATA_STORAGE_KEY)
    if (raw === null) return createEmptyMetadata()
    return sanitizeMetadata(JSON.parse(raw))
  } catch {
    return createEmptyMetadata()
  }
}

export function saveTerminalMetadata(value: TerminalMetadataStore): void {
  try {
    localStorage.setItem(
      TERMINAL_METADATA_STORAGE_KEY,
      JSON.stringify(sanitizeMetadata(value)),
    )
  } catch {
    // Terminal layout metadata must never block the rest of the application.
  }
}

export function clearTerminalMetadata(): void {
  try {
    localStorage.removeItem(TERMINAL_METADATA_STORAGE_KEY)
  } catch {
    // localStorage may be unavailable in restricted browser contexts.
  }
}
