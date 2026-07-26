import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { conversationWorkspaceFilesQueryKey } from '@/hooks/useConversationWorkspaceFiles'
import { fetchJson } from '@/lib/api-v2/client'
import { useAuthStore } from '@/stores/authStore'
import type {
  ConversationScope,
  GroupWorkspaceGitBranchCreateRequest,
  GroupWorkspaceGitBranchDeleteRequest,
  GroupWorkspaceGitBranchRenameRequest,
  GroupWorkspaceGitBranches,
  GroupWorkspaceGitBranchSwitchRequest,
  GroupWorkspaceGitCommitDetails,
  GroupWorkspaceGitCommitMessageResponse,
  GroupWorkspaceGitCommitRequest,
  GroupWorkspaceGitCreateBranchFromCommitRequest,
  GroupWorkspaceGitDiff,
  GroupWorkspaceGitDiffMode,
  GroupWorkspaceGitDiscardRequest,
  GroupWorkspaceGitIgnoreRequest,
  GroupWorkspaceGitInitRequest,
  GroupWorkspaceGitLog,
  GroupWorkspaceGitPathsRequest,
  GroupWorkspaceGitRemoteRequest,
  GroupWorkspaceGitStashPushRequest,
  GroupWorkspaceGitStatus,
} from '@/types/api'

export function workspaceGitQueryKey(groupId: string | undefined) {
  return ['groups', groupId, 'workspace-git'] as const
}

export function workspaceGitDiffQueryKey(
  groupId: string | undefined,
  mode: Exclude<GroupWorkspaceGitDiffMode, 'commit'>,
  path?: string | null,
) {
  return [...workspaceGitQueryKey(groupId), 'diff', mode, path ?? null] as const
}

export function workspaceGitLogQueryKey(groupId: string | undefined) {
  return [...workspaceGitQueryKey(groupId), 'log'] as const
}

export function workspaceGitCommitQueryKey(groupId: string | undefined, sha: string | undefined) {
  return [...workspaceGitQueryKey(groupId), 'commits', sha] as const
}

export function workspaceGitCommitDiffQueryKey(
  groupId: string | undefined,
  sha: string | undefined,
  path?: string | null,
) {
  return [...workspaceGitCommitQueryKey(groupId, sha), 'diff', path ?? null] as const
}

export function workspaceGitBranchesQueryKey(groupId: string | undefined) {
  return [...workspaceGitQueryKey(groupId), 'branches'] as const
}

function queryString(params: Record<string, string | number | null | undefined>) {
  const search = new URLSearchParams()
  for (const [name, value] of Object.entries(params)) {
    if (value !== null && value !== undefined) search.set(name, String(value))
  }
  return search.toString()
}

function requireGroupId(groupId: string | undefined) {
  if (!groupId) throw new Error('Group is required for workspace Git operations')
  return groupId
}

export function useGroupWorkspaceGitStatus(groupId: string | undefined) {
  const token = useAuthStore((state) => state.token)
  return useQuery({
    queryKey: workspaceGitQueryKey(groupId),
    queryFn: () =>
      fetchJson<GroupWorkspaceGitStatus>(`/groups/${requireGroupId(groupId)}/workspace-git/status`, {
        token,
      }),
    enabled: token !== null && !!groupId,
    refetchInterval: 10_000,
  })
}

export function useGroupWorkspaceGitDiff(
  groupId: string | undefined,
  mode: Exclude<GroupWorkspaceGitDiffMode, 'commit'>,
  path?: string | null,
) {
  const token = useAuthStore((state) => state.token)
  return useQuery({
    queryKey: workspaceGitDiffQueryKey(groupId, mode, path),
    queryFn: () => {
      const search = queryString({ mode, path })
      return fetchJson<GroupWorkspaceGitDiff>(
        `/groups/${requireGroupId(groupId)}/workspace-git/diff?${search}`,
        { token },
      )
    },
    enabled: token !== null && !!groupId,
  })
}

export function useGroupWorkspaceGitLog(
  groupId: string | undefined,
  options: { limit?: number; skip?: number } = {},
) {
  const token = useAuthStore((state) => state.token)
  const { limit = 50, skip = 0 } = options
  return useQuery({
    queryKey: [...workspaceGitLogQueryKey(groupId), limit, skip],
    queryFn: () => {
      const search = queryString({ limit, skip })
      return fetchJson<GroupWorkspaceGitLog>(
        `/groups/${requireGroupId(groupId)}/workspace-git/log?${search}`,
        { token },
      )
    },
    enabled: token !== null && !!groupId,
  })
}

export function useGroupWorkspaceGitCommit(groupId: string | undefined, sha: string | undefined) {
  const token = useAuthStore((state) => state.token)
  return useQuery({
    queryKey: workspaceGitCommitQueryKey(groupId, sha),
    queryFn: () =>
      fetchJson<GroupWorkspaceGitCommitDetails>(
        `/groups/${requireGroupId(groupId)}/workspace-git/commits/${sha}`,
        { token },
      ),
    enabled: token !== null && !!groupId && !!sha,
  })
}

export function useGroupWorkspaceGitCommitDiff(
  groupId: string | undefined,
  sha: string | undefined,
  path?: string | null,
) {
  const token = useAuthStore((state) => state.token)
  return useQuery({
    queryKey: workspaceGitCommitDiffQueryKey(groupId, sha, path),
    queryFn: () => {
      const search = queryString({ path })
      const suffix = search ? `?${search}` : ''
      return fetchJson<GroupWorkspaceGitDiff>(
        `/groups/${requireGroupId(groupId)}/workspace-git/commits/${sha}/diff${suffix}`,
        { token },
      )
    },
    enabled: token !== null && !!groupId && !!sha,
  })
}

export function useGroupWorkspaceGitBranches(groupId: string | undefined) {
  const token = useAuthStore((state) => state.token)
  return useQuery({
    queryKey: workspaceGitBranchesQueryKey(groupId),
    queryFn: () =>
      fetchJson<GroupWorkspaceGitBranches>(
        `/groups/${requireGroupId(groupId)}/workspace-git/branches`,
        { token },
      ),
    enabled: token !== null && !!groupId,
  })
}

type WorkspaceGitMutationOptions = {
  invalidateBranches?: boolean
  invalidateDiffs?: boolean
  invalidateFiles?: boolean
}

function useWorkspaceGitMutation<TBody, TResult = GroupWorkspaceGitStatus>(
  groupId: string | undefined,
  endpoint: string,
  options: WorkspaceGitMutationOptions = {},
  scope: ConversationScope = 'groups',
) {
  const token = useAuthStore((state) => state.token)
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body: TBody) =>
      fetchJson<TResult>(`/groups/${requireGroupId(groupId)}/workspace-git/${endpoint}`, {
        token,
        method: 'POST',
        body,
      }),
    onSuccess: () => {
      void queryClient.invalidateQueries({
        queryKey: workspaceGitQueryKey(groupId),
        exact: true,
      })
      if (options.invalidateBranches) {
        void queryClient.invalidateQueries({ queryKey: workspaceGitBranchesQueryKey(groupId) })
        void queryClient.invalidateQueries({ queryKey: workspaceGitLogQueryKey(groupId) })
      }
      if (options.invalidateDiffs) {
        void queryClient.invalidateQueries({
          queryKey: [...workspaceGitQueryKey(groupId), 'diff'],
        })
      }
      if (options.invalidateFiles) {
        void queryClient.invalidateQueries({
          queryKey: conversationWorkspaceFilesQueryKey(scope, groupId),
        })
      }
    },
  })
}

export function useStageGroupWorkspaceGit(groupId: string | undefined) {
  return useWorkspaceGitMutation<GroupWorkspaceGitPathsRequest>(groupId, 'stage', {
    invalidateDiffs: true,
  })
}

export function useUnstageGroupWorkspaceGit(groupId: string | undefined) {
  return useWorkspaceGitMutation<GroupWorkspaceGitPathsRequest>(groupId, 'unstage', {
    invalidateDiffs: true,
  })
}

export function useCommitGroupWorkspaceGit(groupId: string | undefined) {
  return useWorkspaceGitMutation<GroupWorkspaceGitCommitRequest>(groupId, 'commit', {
    invalidateDiffs: true,
  })
}

export function useGenerateGroupWorkspaceGitCommitMessage(groupId: string | undefined) {
  const token = useAuthStore((state) => state.token)
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: () =>
      fetchJson<GroupWorkspaceGitCommitMessageResponse>(
        `/groups/${requireGroupId(groupId)}/workspace-git/commit-message`,
        { token, method: 'POST' },
      ),
    onSuccess: () => {
      void queryClient.invalidateQueries({
        queryKey: workspaceGitQueryKey(groupId),
        exact: true,
      })
    },
  })
}

export function usePullGroupWorkspaceGit(
  groupId: string | undefined,
  scope: ConversationScope = 'groups',
) {
  return useWorkspaceGitMutation<Record<string, never>>(
    groupId,
    'pull',
    { invalidateFiles: true },
    scope,
  )
}

export function usePushGroupWorkspaceGit(groupId: string | undefined) {
  return useWorkspaceGitMutation<Record<string, never>>(groupId, 'push')
}

export function useFetchGroupWorkspaceGit(groupId: string | undefined) {
  return useWorkspaceGitMutation<Record<string, never>>(groupId, 'fetch', { invalidateBranches: true })
}

export function useCreateGroupWorkspaceGitBranch(groupId: string | undefined) {
  return useWorkspaceGitMutation<GroupWorkspaceGitBranchCreateRequest>(groupId, 'branches', {
    invalidateBranches: true,
  })
}

export function useSwitchGroupWorkspaceGitBranch(
  groupId: string | undefined,
  scope: ConversationScope = 'groups',
) {
  return useWorkspaceGitMutation<GroupWorkspaceGitBranchSwitchRequest>(groupId, 'branches/switch', {
    invalidateBranches: true,
    invalidateFiles: true,
  }, scope)
}

export function useRenameGroupWorkspaceGitBranch(groupId: string | undefined) {
  return useWorkspaceGitMutation<GroupWorkspaceGitBranchRenameRequest>(groupId, 'branches/rename', {
    invalidateBranches: true,
  })
}

export function useDeleteGroupWorkspaceGitBranch(groupId: string | undefined) {
  return useWorkspaceGitMutation<GroupWorkspaceGitBranchDeleteRequest>(groupId, 'branches/delete', {
    invalidateBranches: true,
  })
}

export function useCreateGroupWorkspaceGitBranchFromCommit(
  groupId: string | undefined,
  sha: string | undefined,
) {
  const token = useAuthStore((state) => state.token)
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body: GroupWorkspaceGitCreateBranchFromCommitRequest) =>
      fetchJson<void>(
        `/groups/${requireGroupId(groupId)}/workspace-git/commits/${sha}/create-branch`,
        { token, method: 'POST', body },
      ),
    onSuccess: () => {
      void queryClient.invalidateQueries({
        queryKey: workspaceGitQueryKey(groupId),
        exact: true,
      })
      void queryClient.invalidateQueries({ queryKey: workspaceGitBranchesQueryKey(groupId) })
      void queryClient.invalidateQueries({ queryKey: workspaceGitLogQueryKey(groupId) })
    },
  })
}

export function useInitGroupWorkspaceGit(groupId: string | undefined) {
  return useWorkspaceGitMutation<GroupWorkspaceGitInitRequest>(groupId, 'init')
}

export function useSetGroupWorkspaceGitRemote(groupId: string | undefined) {
  return useWorkspaceGitMutation<GroupWorkspaceGitRemoteRequest>(groupId, 'set-remote')
}

export function useDiscardGroupWorkspaceGit(
  groupId: string | undefined,
  scope: ConversationScope = 'groups',
) {
  return useWorkspaceGitMutation<GroupWorkspaceGitDiscardRequest>(groupId, 'discard', {
    invalidateDiffs: true,
    invalidateFiles: true,
  }, scope)
}

export function useIgnoreGroupWorkspaceGit(groupId: string | undefined) {
  return useWorkspaceGitMutation<GroupWorkspaceGitIgnoreRequest>(groupId, 'ignore')
}

export function usePushGroupWorkspaceGitStash(
  groupId: string | undefined,
  scope: ConversationScope = 'groups',
) {
  return useWorkspaceGitMutation<GroupWorkspaceGitStashPushRequest>(groupId, 'stash/push', {
    invalidateDiffs: true,
    invalidateFiles: true,
  }, scope)
}

export function usePopGroupWorkspaceGitStash(
  groupId: string | undefined,
  scope: ConversationScope = 'groups',
) {
  return useWorkspaceGitMutation<Record<string, never>>(groupId, 'stash/pop', {
    invalidateDiffs: true,
    invalidateFiles: true,
  }, scope)
}
