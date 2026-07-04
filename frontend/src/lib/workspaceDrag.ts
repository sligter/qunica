export const WORKSPACE_PATHS_MIME = 'application/x-ag-swarmer-workspace-paths'

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
