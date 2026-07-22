export const TERMINAL_METADATA_STORAGE_KEY = 'ag-swarmer:terminal-metadata:v1'

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

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function sanitizeTab(value: unknown): TerminalTabMetadata | null {
  if (!isRecord(value)) return null
  if (
    typeof value.id !== 'string' ||
    typeof value.label !== 'string' ||
    typeof value.launchDirectory !== 'string'
  ) {
    return null
  }
  return {
    id: value.id,
    label: value.label,
    launchDirectory: value.launchDirectory,
  }
}

function sanitizeConversation(value: unknown): TerminalConversationMetadata | null {
  if (!isRecord(value)) return null
  const tabs = Array.isArray(value.tabs)
    ? value.tabs.flatMap((tab) => {
        const sanitized = sanitizeTab(tab)
        return sanitized === null ? [] : [sanitized]
      })
    : []
  return {
    open: typeof value.open === 'boolean' ? value.open : false,
    activeTabId:
      typeof value.activeTabId === 'string' || value.activeTabId === null
        ? value.activeTabId
        : null,
    tabs,
  }
}

function sanitizeMetadata(value: unknown): TerminalMetadataStore {
  if (!isRecord(value)) return { ...EMPTY_METADATA, conversations: {} }

  const conversationEntries: [string, TerminalConversationMetadata][] = []
  if (isRecord(value.conversations)) {
    for (const [conversationId, conversation] of Object.entries(value.conversations)) {
      const sanitized = sanitizeConversation(conversation)
      if (sanitized !== null) conversationEntries.push([conversationId, sanitized])
    }
  }

  return {
    height: typeof value.height === 'number' && Number.isFinite(value.height) ? value.height : 0,
    conversations: Object.fromEntries(conversationEntries),
  }
}

export function loadTerminalMetadata(): TerminalMetadataStore {
  try {
    const raw = localStorage.getItem(TERMINAL_METADATA_STORAGE_KEY)
    if (raw === null) return { ...EMPTY_METADATA, conversations: {} }
    return sanitizeMetadata(JSON.parse(raw))
  } catch {
    return { ...EMPTY_METADATA, conversations: {} }
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
