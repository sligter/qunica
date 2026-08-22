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
  GroupWorkspaceGitCommitMessageRequest,
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

export function workspaceGitQueryKey(groupId: string | undefined, threadId?: string) {
  const root = ['groups', groupId, 'workspace-git'] as const
  return threadId ? [...root, 'thread', threadId] as const : root
}

export function workspaceGitDiffQueryKey(
  groupId: string | undefined,
  mode: Exclude<GroupWorkspaceGitDiffMode, 'commit'>,
  path?: string | null,
  threadId?: string,
) {
  return [...workspaceGitQueryKey(groupId, threadId), 'diff', mode, path ?? null] as const
}

export function workspaceGitLogQueryKey(groupId: string | undefined, threadId?: string) {
  return [...workspaceGitQueryKey(groupId, threadId), 'log'] as const
}

export function workspaceGitCommitQueryKey(
  groupId: string | undefined,
  sha: string | undefined,
  threadId?: string,
) {
  return [...workspaceGitQueryKey(groupId, threadId), 'commits', sha] as const
}

export function workspaceGitCommitDiffQueryKey(
  groupId: string | undefined,
  sha: string | undefined,
  path?: string | null,
  threadId?: string,
) {
  return [...workspaceGitCommitQueryKey(groupId, sha, threadId), 'diff', path ?? null] as const
}

export function workspaceGitBranchesQueryKey(groupId: string | undefined, threadId?: string) {
  return [...workspaceGitQueryKey(groupId, threadId), 'branches'] as const
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

function workspaceGitPath(
  groupId: string | undefined,
  endpoint: string,
  params: Record<string, string | number | null | undefined> = {},
) {
  const search = queryString(params)
  const path = `/groups/${requireGroupId(groupId)}/workspace-git/${endpoint}`
  return search ? `${path}?${search}` : path
}

function isWorkspaceGitStatus(value: unknown): value is GroupWorkspaceGitStatus {
  return typeof value === 'object'
    && value !== null
    && typeof (value as GroupWorkspaceGitStatus).available === 'boolean'
    && Array.isArray((value as GroupWorkspaceGitStatus).files)
}

export function useGroupWorkspaceGitStatus(groupId: string | undefined, threadId?: string) {
  const token = useAuthStore((state) => state.token)
  return useQuery({
    queryKey: workspaceGitQueryKey(groupId, threadId),
    queryFn: () =>
      fetchJson<GroupWorkspaceGitStatus>(workspaceGitPath(groupId, 'status', { thread_id: threadId }), {
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
  threadId?: string,
) {
  const token = useAuthStore((state) => state.token)
  return useQuery({
    queryKey: workspaceGitDiffQueryKey(groupId, mode, path, threadId),
    queryFn: () => {
      return fetchJson<GroupWorkspaceGitDiff>(
        workspaceGitPath(groupId, 'diff', { mode, path, thread_id: threadId }),
        { token },
      )
    },
    enabled: token !== null && !!groupId,
  })
}

export function useGroupWorkspaceGitLog(
  groupId: string | undefined,
  options: { limit?: number; skip?: number } = {},
  threadId?: string,
) {
  const token = useAuthStore((state) => state.token)
  const { limit = 50, skip = 0 } = options
  return useQuery({
    queryKey: [...workspaceGitLogQueryKey(groupId, threadId), limit, skip],
    queryFn: () => {
      return fetchJson<GroupWorkspaceGitLog>(
        workspaceGitPath(groupId, 'log', { limit, skip, thread_id: threadId }),
        { token },
      )
    },
    enabled: token !== null && !!groupId,
  })
}

export function useGroupWorkspaceGitCommit(
  groupId: string | undefined,
  sha: string | undefined,
  threadId?: string,
) {
  const token = useAuthStore((state) => state.token)
  return useQuery({
    queryKey: workspaceGitCommitQueryKey(groupId, sha, threadId),
    queryFn: () =>
      fetchJson<GroupWorkspaceGitCommitDetails>(
        workspaceGitPath(groupId, `commits/${sha}`, { thread_id: threadId }),
        { token },
      ),
    enabled: token !== null && !!groupId && !!sha,
  })
}

export function useGroupWorkspaceGitCommitDiff(
  groupId: string | undefined,
  sha: string | undefined,
  path?: string | null,
  threadId?: string,
) {
  const token = useAuthStore((state) => state.token)
  return useQuery({
    queryKey: workspaceGitCommitDiffQueryKey(groupId, sha, path, threadId),
    queryFn: () => {
      return fetchJson<GroupWorkspaceGitDiff>(
        workspaceGitPath(groupId, `commits/${sha}/diff`, { path, thread_id: threadId }),
        { token },
      )
    },
    enabled: token !== null && !!groupId && !!sha,
  })
}

export function useGroupWorkspaceGitBranches(groupId: string | undefined, threadId?: string) {
  const token = useAuthStore((state) => state.token)
  return useQuery({
    queryKey: workspaceGitBranchesQueryKey(groupId, threadId),
    queryFn: () =>
      fetchJson<GroupWorkspaceGitBranches>(
        workspaceGitPath(groupId, 'branches', { thread_id: threadId }),
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
  threadId?: string,
  scope: ConversationScope = 'groups',
) {
  const token = useAuthStore((state) => state.token)
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body: TBody) =>
      fetchJson<TResult>(workspaceGitPath(groupId, endpoint, { thread_id: threadId }), {
        token,
        method: 'POST',
        body,
      }),
    onSuccess: (result) => {
      if (isWorkspaceGitStatus(result)) {
        queryClient.setQueryData(workspaceGitQueryKey(groupId, threadId), result)
      } else {
        void queryClient.invalidateQueries({
          queryKey: workspaceGitQueryKey(groupId, threadId),
          exact: true,
        })
      }
      if (options.invalidateBranches) {
        void queryClient.invalidateQueries({ queryKey: workspaceGitBranchesQueryKey(groupId, threadId) })
        void queryClient.invalidateQueries({ queryKey: workspaceGitLogQueryKey(groupId, threadId) })
      }
      if (options.invalidateDiffs) {
        void queryClient.invalidateQueries({
          queryKey: [...workspaceGitQueryKey(groupId, threadId), 'diff'],
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

export function useStageGroupWorkspaceGit(groupId: string | undefined, threadId?: string) {
  return useWorkspaceGitMutation<GroupWorkspaceGitPathsRequest>(groupId, 'stage', {
    invalidateDiffs: true,
  }, threadId)
}

export function useUnstageGroupWorkspaceGit(groupId: string | undefined, threadId?: string) {
  return useWorkspaceGitMutation<GroupWorkspaceGitPathsRequest>(groupId, 'unstage', {
    invalidateDiffs: true,
  }, threadId)
}

export function useCommitGroupWorkspaceGit(groupId: string | undefined, threadId?: string) {
  return useWorkspaceGitMutation<GroupWorkspaceGitCommitRequest>(groupId, 'commit', {
    invalidateBranches: true,
    invalidateDiffs: true,
  }, threadId)
}

export function useGenerateGroupWorkspaceGitCommitMessage(
  groupId: string | undefined,
  threadId?: string,
) {
  const token = useAuthStore((state) => state.token)
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body: GroupWorkspaceGitCommitMessageRequest) =>
      fetchJson<GroupWorkspaceGitCommitMessageResponse>(
        workspaceGitPath(groupId, 'commit-message', { thread_id: threadId }),
        { token, method: 'POST', body },
      ),
    onSuccess: () => {
      void queryClient.invalidateQueries({
        queryKey: workspaceGitQueryKey(groupId, threadId),
        exact: true,
      })
    },
  })
}

export function usePullGroupWorkspaceGit(
  groupId: string | undefined,
  scope: ConversationScope = 'groups',
  threadId?: string,
) {
  return useWorkspaceGitMutation<Record<string, never>>(
    groupId,
    'pull',
    { invalidateBranches: true, invalidateDiffs: true, invalidateFiles: true },
    threadId,
    scope,
  )
}

export function usePushGroupWorkspaceGit(groupId: string | undefined, threadId?: string) {
  return useWorkspaceGitMutation<Record<string, never>>(groupId, 'push', {}, threadId)
}

export function useForcePushGroupWorkspaceGit(groupId: string | undefined, threadId?: string) {
  return useWorkspaceGitMutation<Record<string, never>>(groupId, 'force-push', {}, threadId)
}

export function useRebaseGroupWorkspaceGit(
  groupId: string | undefined,
  scope: ConversationScope = 'groups',
  threadId?: string,
) {
  return useWorkspaceGitMutation<Record<string, never>>(
    groupId,
    'rebase',
    { invalidateBranches: true, invalidateDiffs: true, invalidateFiles: true },
    threadId,
    scope,
  )
}

export function useFetchGroupWorkspaceGit(groupId: string | undefined, threadId?: string) {
  return useWorkspaceGitMutation<Record<string, never>>(
    groupId,
    'fetch',
    { invalidateBranches: true },
    threadId,
  )
}

export function useCreateGroupWorkspaceGitBranch(groupId: string | undefined, threadId?: string) {
  return useWorkspaceGitMutation<GroupWorkspaceGitBranchCreateRequest, GroupWorkspaceGitBranches>(groupId, 'branches', {
    invalidateBranches: true,
  }, threadId)
}

export function useSwitchGroupWorkspaceGitBranch(
  groupId: string | undefined,
  scope: ConversationScope = 'groups',
  threadId?: string,
) {
  return useWorkspaceGitMutation<GroupWorkspaceGitBranchSwitchRequest>(groupId, 'branches/switch', {
    invalidateBranches: true,
    invalidateFiles: true,
  }, threadId, scope)
}

export function useRenameGroupWorkspaceGitBranch(groupId: string | undefined, threadId?: string) {
  return useWorkspaceGitMutation<GroupWorkspaceGitBranchRenameRequest, GroupWorkspaceGitBranches>(groupId, 'branches/rename', {
    invalidateBranches: true,
  }, threadId)
}

export function useDeleteGroupWorkspaceGitBranch(groupId: string | undefined, threadId?: string) {
  return useWorkspaceGitMutation<GroupWorkspaceGitBranchDeleteRequest, GroupWorkspaceGitBranches>(groupId, 'branches/delete', {
    invalidateBranches: true,
  }, threadId)
}

export function useCreateGroupWorkspaceGitBranchFromCommit(
  groupId: string | undefined,
  sha: string | undefined,
  threadId?: string,
) {
  const token = useAuthStore((state) => state.token)
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body: GroupWorkspaceGitCreateBranchFromCommitRequest) =>
      fetchJson<void>(
        workspaceGitPath(groupId, `commits/${sha}/create-branch`, { thread_id: threadId }),
        { token, method: 'POST', body },
      ),
    onSuccess: () => {
      void queryClient.invalidateQueries({
        queryKey: workspaceGitQueryKey(groupId, threadId),
        exact: true,
      })
      void queryClient.invalidateQueries({ queryKey: workspaceGitBranchesQueryKey(groupId, threadId) })
      void queryClient.invalidateQueries({ queryKey: workspaceGitLogQueryKey(groupId, threadId) })
    },
  })
}

export function useInitGroupWorkspaceGit(groupId: string | undefined, threadId?: string) {
  return useWorkspaceGitMutation<GroupWorkspaceGitInitRequest>(groupId, 'init', {}, threadId)
}

export function useSetGroupWorkspaceGitRemote(groupId: string | undefined, threadId?: string) {
  return useWorkspaceGitMutation<GroupWorkspaceGitRemoteRequest>(
    groupId,
    'set-remote',
    {},
    threadId,
  )
}

export function useDiscardGroupWorkspaceGit(
  groupId: string | undefined,
  scope: ConversationScope = 'groups',
  threadId?: string,
) {
  return useWorkspaceGitMutation<GroupWorkspaceGitDiscardRequest>(groupId, 'discard', {
    invalidateDiffs: true,
    invalidateFiles: true,
  }, threadId, scope)
}

export function useIgnoreGroupWorkspaceGit(groupId: string | undefined, threadId?: string) {
  return useWorkspaceGitMutation<GroupWorkspaceGitIgnoreRequest>(groupId, 'ignore', {}, threadId)
}

export function usePushGroupWorkspaceGitStash(
  groupId: string | undefined,
  scope: ConversationScope = 'groups',
  threadId?: string,
) {
  return useWorkspaceGitMutation<GroupWorkspaceGitStashPushRequest>(groupId, 'stash/push', {
    invalidateDiffs: true,
    invalidateFiles: true,
  }, threadId, scope)
}

export function usePopGroupWorkspaceGitStash(
  groupId: string | undefined,
  scope: ConversationScope = 'groups',
  threadId?: string,
) {
  return useWorkspaceGitMutation<Record<string, never>>(groupId, 'stash/pop', {
    invalidateDiffs: true,
    invalidateFiles: true,
  }, threadId, scope)
}
