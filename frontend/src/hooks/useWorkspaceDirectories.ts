import { useQuery } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api-v2/client'
import { useAuthStore } from '@/stores/authStore'

export interface WorkspaceDirectoryEntry {
  name: string
  relative_path: string
  absolute_path: string
}

export interface WorkspaceDirectoryListing {
  root: string
  absolute_path: string
  relative_path: string
  parent_relative_path: string | null
  entries: WorkspaceDirectoryEntry[]
  truncated: boolean
}

/**
 * Directories under the account's workspace root, listed by the backend.
 *
 * The OS folder picker can only ever show the machine running the browser, so
 * a deployment where the server is somewhere else — a container, a VPS — has
 * to ask the server what it has.
 */
export function useWorkspaceDirectories(relativePath: string, enabled: boolean) {
  const token = useAuthStore((s) => s.token)
  return useQuery({
    queryKey: ['workspace-directories', relativePath],
    queryFn: () =>
      fetchJson<WorkspaceDirectoryListing>(
        `/workspaces/directories?path=${encodeURIComponent(relativePath)}`,
        { token },
      ),
    enabled: enabled && token !== null,
    // A directory the user just created elsewhere in the app should show up.
    staleTime: 0,
  })
}
