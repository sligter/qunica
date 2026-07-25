export const WORKSPACE_PATHS_MIME = 'application/x-ag-swarmer-workspace-paths'
export const WORKSPACE_ITEM_MIME = 'application/x-ag-swarmer-workspace-item+json'
export const WORKSPACE_DRAG_ITEM_VERSION = 1 as const

export type WorkspaceDragItemKind = 'file' | 'directory'

export interface WorkspaceDragItem {
  version: typeof WORKSPACE_DRAG_ITEM_VERSION
  path: string
  name: string
  kind: WorkspaceDragItemKind
}

export type WorkspaceDragItemInput = Omit<WorkspaceDragItem, 'version'>

export interface WorkspaceDropItems {
  files: WorkspaceDragItem[]
  directories: WorkspaceDragItem[]
}

const WORKSPACE_DRAG_ITEM_KEYS = ['kind', 'name', 'path', 'version'] as const
const WINDOWS_DRIVE_PATTERN = /^[a-zA-Z]:/

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function hasExactDragItemKeys(value: Record<string, unknown>): boolean {
  const keys = Object.keys(value).sort()
  return keys.length === WORKSPACE_DRAG_ITEM_KEYS.length
    && keys.every((key, index) => key === WORKSPACE_DRAG_ITEM_KEYS[index])
}

function hasControlCharacter(value: string): boolean {
  return Array.from(value).some((character) => {
    const codePoint = character.codePointAt(0)
    return codePoint !== undefined && (codePoint <= 0x1f || codePoint === 0x7f)
  })
}

export function isWorkspaceRelativePath(path: string): boolean {
  if (!path || path.trim().length === 0 || hasControlCharacter(path)) return false
  if (path.startsWith('/') || path.startsWith('\\') || WINDOWS_DRIVE_PATTERN.test(path)) {
    return false
  }
  if (path.includes('\\')) return false

  const segments = path.split('/')
  return segments.every(
    (segment) => segment.length > 0 && segment !== '.' && segment !== '..' && segment !== '~',
  )
}

function isWorkspaceItemName(name: string, path: string): boolean {
  if (!name || name.trim().length === 0 || hasControlCharacter(name)) return false
  if (name.includes('/') || name.includes('\\')) return false
  return path.split('/').at(-1) === name
}

function workspaceDragItemFromValue(value: unknown): WorkspaceDragItem | null {
  if (!isRecord(value) || !hasExactDragItemKeys(value)) return null
  if (value.version !== WORKSPACE_DRAG_ITEM_VERSION) return null
  if (typeof value.path !== 'string' || !isWorkspaceRelativePath(value.path)) return null
  if (typeof value.name !== 'string' || !isWorkspaceItemName(value.name, value.path)) return null
  if (value.kind !== 'file' && value.kind !== 'directory') return null

  return {
    version: WORKSPACE_DRAG_ITEM_VERSION,
    path: value.path,
    name: value.name,
    kind: value.kind,
  }
}

export function encodeWorkspaceDragItem(item: WorkspaceDragItemInput): string {
  const versionedItem: WorkspaceDragItem = {
    version: WORKSPACE_DRAG_ITEM_VERSION,
    ...item,
  }
  if (!workspaceDragItemFromValue(versionedItem)) {
    throw new TypeError('Invalid workspace drag item')
  }
  return JSON.stringify(versionedItem)
}

export function encodeWorkspaceDragItems(items: readonly WorkspaceDragItemInput[]): string {
  return JSON.stringify(items.map((item) => JSON.parse(encodeWorkspaceDragItem(item))))
}

export function decodeWorkspaceDragItem(raw: string): WorkspaceDragItem | null {
  if (!raw.trim()) return null
  try {
    return workspaceDragItemFromValue(JSON.parse(raw))
  } catch {
    return null
  }
}

function decodeWorkspaceDragItems(raw: string): WorkspaceDragItem[] {
  if (!raw.trim()) return []
  try {
    const parsed: unknown = JSON.parse(raw)
    const values = Array.isArray(parsed) ? parsed : [parsed]
    return values
      .map(workspaceDragItemFromValue)
      .filter((item): item is WorkspaceDragItem => item !== null)
  } catch {
    return []
  }
}

export function workspaceItemsFromDataTransfer(dataTransfer: DataTransfer): WorkspaceDropItems {
  const raw = dataTransfer.getData(WORKSPACE_ITEM_MIME)
  const files: WorkspaceDragItem[] = []
  const directories: WorkspaceDragItem[] = []
  const seenPaths = new Set<string>()

  for (const item of decodeWorkspaceDragItems(raw)) {
    if (seenPaths.has(item.path)) continue
    seenPaths.add(item.path)
    if (item.kind === 'file') files.push(item)
    else directories.push(item)
  }

  return { files, directories }
}

export function encodeWorkspacePaths(paths: string[]): string {
  return JSON.stringify(paths.filter((path) => path.trim().length > 0))
}

export function decodeWorkspacePaths(raw: string): string[] {
  const trimmed = raw.trim()
  if (!trimmed) return []
  if (trimmed.startsWith('[')) {
    try {
      const parsed: unknown = JSON.parse(trimmed)
      if (Array.isArray(parsed)) {
        return parsed
          .filter((item): item is string => typeof item === 'string')
          .map((item) => item.trim())
          .filter((item) => item.length > 0)
      }
    } catch {
      return []
    }
  }
  return trimmed
    .split(/\r?\n/)
    .map((path) => path.trim())
    .filter((path) => path.length > 0)
}

export function workspacePathsFromDataTransfer(dataTransfer: DataTransfer): string[] {
  const custom = dataTransfer.getData(WORKSPACE_PATHS_MIME)
  if (custom) return decodeWorkspacePaths(custom)
  return decodeWorkspacePaths(dataTransfer.getData('text/plain'))
}
