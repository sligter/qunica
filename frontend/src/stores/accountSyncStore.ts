import { create } from 'zustand'

export interface AccountJob {
  id: string
  group_id: string
  thread_id: string
  turn_id: string
  stream_id: string
  status: 'queued' | 'running' | 'waiting_for_user'
}
export interface AccountSnapshot {
  blocked_jobs?: { id: string; group_id: string; thread_id: string; turn_id: string }[]
  server_id: string
  cursor: string
  groups: { id: string; name: string; conversation_kind: string }[]
  threads: { id: string; group_id: string; title: string; status: string }[]
  active_jobs: AccountJob[]
  pending_approvals: { id: string; group_id: string; thread_id: string; tool_call_id: string; version: number; action_hash: string }[]
}

// Note: cursors live only with this in-memory state; an app restart takes a new snapshot.
export const useAccountSyncStore = create<{
  snapshot: AccountSnapshot | null
  status: 'connecting' | 'connected' | 'error'
  error: string | null
}>(() => ({ snapshot: null, status: 'connecting', error: null }))
