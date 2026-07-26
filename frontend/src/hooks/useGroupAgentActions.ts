import { useMutation, useQueryClient } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api-v2/client'
import { useAuthStore } from '@/stores/authStore'
import type { GroupAgentRead, GroupTopologyRole, GroupWorkspaceMode } from '@/types/api'

interface AgentMutationVars {
  groupId: string
  agentId: string
}

interface AgentMuteVars extends AgentMutationVars {
  muted: boolean
}

interface AgentWorkspaceModeVars extends AgentMutationVars {
  workspaceMode: GroupWorkspaceMode
}

interface AgentTopologyVars extends AgentMutationVars {
  topologyRole?: GroupTopologyRole | null
  speakingOrder?: number | null
}

export function useRemoveGroupAgent() {
  const qc = useQueryClient()
  const token = useAuthStore((s) => s.token)
  return useMutation({
    mutationFn: ({ groupId, agentId }: AgentMutationVars) =>
      fetchJson<void>(`/groups/${groupId}/agents/${agentId}`, {
        token,
        method: 'DELETE',
      }),
    onSuccess: (_data, { groupId }) => {
      void qc.invalidateQueries({ queryKey: ['groups', groupId, 'agents'] })
      void qc.invalidateQueries({ queryKey: ['groups', groupId] })
      void qc.invalidateQueries({ queryKey: ['groups'] })
    },
  })
}

export function useSetGroupAgentWorkspaceMode() {
  const qc = useQueryClient()
  const token = useAuthStore((s) => s.token)
  return useMutation({
    mutationFn: ({ groupId, agentId, workspaceMode }: AgentWorkspaceModeVars) =>
      fetchJson<GroupAgentRead>(`/groups/${groupId}/agents/${agentId}/workspace-sharing`, {
        token,
        method: 'PATCH',
        body: { workspace_mode: workspaceMode },
      }),
    onSuccess: (_data, { groupId }) => {
      void qc.invalidateQueries({ queryKey: ['groups', groupId, 'agents'] })
    },
  })
}

export function useSetGroupAgentTopology() {
  const qc = useQueryClient()
  const token = useAuthStore((s) => s.token)
  return useMutation({
    mutationFn: ({ groupId, agentId, topologyRole, speakingOrder }: AgentTopologyVars) =>
      fetchJson<GroupAgentRead>(`/groups/${groupId}/agents/${agentId}/topology`, {
        token,
        method: 'PATCH',
        body: { topology_role: topologyRole, speaking_order: speakingOrder },
      }),
    onSuccess: (_data, { groupId }) => {
      void qc.invalidateQueries({ queryKey: ['groups', groupId, 'agents'] })
      void qc.invalidateQueries({ queryKey: ['groups', groupId] })
    },
  })
}

export function useMuteGroupAgent() {
  const qc = useQueryClient()
  const token = useAuthStore((s) => s.token)
  return useMutation({
    mutationFn: ({ groupId, agentId, muted }: AgentMuteVars) =>
      fetchJson<GroupAgentRead>(`/groups/${groupId}/agents/${agentId}/mute`, {
        token,
        method: 'PATCH',
        body: { muted },
      }),
    onSuccess: (_data, { groupId }) => {
      void qc.invalidateQueries({ queryKey: ['groups', groupId, 'agents'] })
      void qc.invalidateQueries({ queryKey: ['groups', groupId] })
      void qc.invalidateQueries({ queryKey: ['groups'] })
    },
  })
}
