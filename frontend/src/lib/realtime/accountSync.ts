import { fetchEventSource } from '@microsoft/fetch-event-source'
import type { QueryClient } from '@tanstack/react-query'
import { ApiError } from '@/lib/api-v2/client'
import { retryDelayMs } from '@/lib/api-v2/retry'
import { authFetch } from '@/lib/authFetch'
import { LAN_ORIGIN } from '@/lib/androidSession'
import { notifyReplyOutcome } from '@/lib/replyNotifications'
import { useAccountSyncStore, type AccountSnapshot } from '@/stores/accountSyncStore'
import { useConversationActivityStore } from '@/stores/conversationActivityStore'
import { useMessageStore } from '@/stores/messageStore'
import { realtimeClient, validCursor, type RealtimeFrame } from './client'

/** Account discovery does not subscribe to tokens or execute work. */
export function startAccountSync(token: string, queries: QueryClient): () => void {
  const client = realtimeClient(token)
  const signal = client.controller.signal
  let disposed = false
  let generation = 0
  let cursor = '0'
  let snapshot: AccountSnapshot | null = null
  let unsubscribe: (() => void) | undefined
  let sse: AbortController | undefined
  let refreshTimer: ReturnType<typeof setTimeout> | undefined
  let retryTimer: ReturnType<typeof setTimeout> | undefined
  let retry = 0
  const remoteRuns = new Set<string>()
  const seen = new Set<string>()
  const stale = () => disposed || signal.aborted
  const cleanupConnection = client.origin === LAN_ORIGIN ? undefined : client.observeConnection?.(status => {
    if (!stale()) useAccountSyncStore.setState({ status })
  })

  function reflectSnapshot(next: AccountSnapshot) {
    if (snapshot && BigInt(next.cursor) < BigInt(snapshot.cursor)) return
    snapshot = next
    useAccountSyncStore.setState({ snapshot: next, error: null })
    const activity = useConversationActivityStore.getState()
    const current = new Set<string>()
    for (const job of next.active_jobs) {
      const group = next.groups.find(item => item.id === job.group_id)
      const thread = next.threads.find(item => item.id === job.thread_id)
      const threadId = group?.conversation_kind === 'direct' ? null : job.thread_id
      activity.registerConversationTitles(job.group_id, threadId, { conversation: group?.name ?? null, thread: thread?.title ?? null })
      // Local streams already own their notification and run lifecycle.
      if (client.localRuns.has(job.stream_id) || client.localContinuationThreads.has(job.thread_id)
        || (activity.runs[job.stream_id] && !remoteRuns.has(job.stream_id))) continue
      current.add(job.stream_id)
      if (!remoteRuns.has(job.stream_id)) {
        activity.startRun({ id: job.stream_id, conversationId: job.group_id, threadId, scope: group?.conversation_kind === 'direct' ? 'direct-chats' : 'groups' })
      }
      if (job.status === 'waiting_for_user') activity.markRunWaiting(job.stream_id)
    }
    for (const id of remoteRuns) if (!current.has(id)) activity.finishRun(id, 'completed')
    remoteRuns.clear()
    for (const id of current) remoteRuns.add(id)
  }

  function refreshSummaries() {
    if (refreshTimer || stale()) return
    refreshTimer = setTimeout(() => {
      refreshTimer = undefined
      const current = generation
      void client.request<AccountSnapshot>('/account/snapshot').then(next => {
        if (!stale() && current === generation) reflectSnapshot(next)
      }).catch(error => { if (!stale()) useAccountSyncStore.setState({ error: String(error) }) })
    }, 50)
  }

  function apply(event: RealtimeFrame) {
    if (stale()) return
    if (!validCursor(event.cursor)) throw new Error('Invalid account event cursor')
    if (BigInt(event.cursor) <= BigInt(cursor)) return
    const id = String(event.event_id)
    if (!seen.has(id)) {
      const kind = String(event.kind)
      const groupId = event.group_id
      const payload = event.payload as { status?: string; operation?: string; stream_id?: string; turn_id?: string; conversation_kind?: string } | undefined
      if (kind === 'message.changed' && payload?.operation === 'delete' && typeof groupId === 'string') {
        for (const bucket of [groupId, event.thread_id]) if (typeof bucket === 'string') useMessageStore.getState().removeMessage(bucket, String(event.resource_id))
      }
      if (typeof groupId === 'string' && ((kind === 'thread.changed' && ['cleared', 'archived'].includes(payload?.status ?? ''))
        || (kind === 'group.changed' && ['deleted', 'archived'].includes(payload?.status ?? '')))) {
        const bucket = kind === 'thread.changed' ? event.thread_id : groupId
        const retire = payload?.status === 'archived' ? useMessageStore.getState().retireStreams : useMessageStore.getState().clearGroupMessages
        if (typeof bucket === 'string') retire(bucket)
        if (kind === 'group.changed') for (const thread of snapshot?.threads.filter(item => item.group_id === groupId) ?? []) retire(thread.id)
      }
      void queries.invalidateQueries({ predicate: query => {
        const root = query.queryKey[0]
        if (groupId && (root === 'groups' || root === 'direct-chats')) return query.queryKey.length === 1 || query.queryKey[1] === groupId
        const roots: Record<string, string[]> = {
          group: ['groups', 'direct-chats', 'computers'], thread: ['groups', 'direct-chats'],
          agent: ['agents', 'groups', 'direct-chats', 'computers'], workspace: ['workspaces', 'groups', 'direct-chats'],
          computer: ['computers', 'computer-control', 'computer-artifacts'], automation: ['automations'],
          provider: ['llm-providers', 'providers'], settings: ['settings'], skill: ['skills'], mcp_server: ['mcp-servers'], profile: ['auth'],
        }
        return (roots[kind.split('.')[0]] ?? []).includes(String(root))
      } })
      if (['job.changed', 'turn.changed', 'approval.changed', 'group.changed', 'thread.changed'].includes(kind)) refreshSummaries()
      if (kind === 'job.changed') {
        const job = snapshot?.active_jobs.find(item => item.id === event.resource_id)
          ?? (payload?.stream_id && typeof groupId === 'string' && typeof event.thread_id === 'string' ? {
            id: String(event.resource_id), stream_id: payload.stream_id, group_id: groupId, thread_id: event.thread_id,
          } : undefined)
        if (job && !client.localRuns.has(job.stream_id) && !client.localContinuationThreads.has(job.thread_id)
          && !useConversationActivityStore.getState().runs[job.stream_id] && payload?.operation !== 'delete') {
          const group = snapshot?.groups.find(item => item.id === groupId)
          const direct = (group?.conversation_kind ?? payload?.conversation_kind) === 'direct'
          useConversationActivityStore.getState().startRun({ id: job.stream_id, conversationId: job.group_id,
            threadId: direct ? null : job.thread_id, scope: direct ? 'direct-chats' : 'groups' })
          remoteRuns.add(job.stream_id)
        }
        if (job && remoteRuns.has(job.stream_id) && ['succeeded', 'failed', 'waiting_for_user', 'cancelled', 'interrupted'].includes(payload?.status ?? '')) {
          const activity = useConversationActivityStore.getState()
          const waiting = payload?.status === 'waiting_for_user'
          const outcome = payload?.status === 'cancelled' ? 'cancelled' : ['failed', 'interrupted'].includes(payload?.status ?? '') ? 'failed' : 'completed'
          const run = waiting ? activity.markRunWaiting(job.stream_id) : activity.finishRun(job.stream_id, outcome)
          if (run && !run.announced && outcome !== 'cancelled') notifyReplyOutcome(run, waiting ? 'waiting' : outcome)
          if (!waiting) remoteRuns.delete(job.stream_id)
        }
      }
      seen.add(id)
      if (seen.size > 4096) seen.delete(seen.values().next().value!)
    }
    cursor = event.cursor
    useAccountSyncStore.setState({ status: 'connected', error: null })
  }

  function pairedSubscribe(serverId: string) {
    sse = new AbortController()
    void fetchEventSource(`${client.origin}/api/v2/account/events?server_id=${encodeURIComponent(serverId)}&cursor=${cursor}`, {
      signal: sse.signal, openWhenHidden: true,
      headers: { Authorization: `Bearer ${token}`, Accept: 'text/event-stream' },
      fetch: (input, init) => {
        if (stale()) throw new DOMException('Session changed', 'AbortError')
        const headers = new Headers(init?.headers)
        headers.set('last-event-id', cursor)
        return authFetch(input, { ...init, headers })
      },
      onopen: async response => {
        if (response.ok) { retry = 0; useAccountSyncStore.setState({ status: 'connected', error: null }); return }
        const value = await response.json().catch(() => null)
        const error = new ApiError(response.status, value?.error?.code ?? 'http_error', value?.error?.message ?? 'Unable to connect')
        if (error.code === 'resync_required') { void start(); return }
        throw error
      },
      onmessage: message => {
        if (stale()) return
        if (message.event === 'resync_required') { void start(); return }
        if (message.data.trim()) apply(JSON.parse(message.data) as RealtimeFrame)
      },
      onclose: () => { throw new TypeError('Account event stream closed') },
      onerror: error => {
        if (stale()) throw error
        if (error instanceof ApiError && [401, 403, 404].includes(error.status)) { failed(error); throw error }
        useAccountSyncStore.setState({ status: 'connecting', error: null })
        return retryDelayMs(++retry)
      },
    }).catch(() => undefined)
  }

  function failed(error: Error) {
    if (stale()) return
    useAccountSyncStore.setState({ status: 'error', error: error.message })
  }

  async function start() {
    const current = ++generation
    unsubscribe?.()
    sse?.abort()
    clearTimeout(retryTimer)
    try {
      useAccountSyncStore.setState({ status: 'connecting', error: null })
      const next = await client.request<AccountSnapshot>('/account/snapshot')
      if (stale() || current !== generation) return
      if (!validCursor(next.cursor)) throw new Error('Invalid snapshot cursor')
      reflectSnapshot(next)
      cursor = next.cursor
      // Existing mounted lists may predate the snapshot. Mark them stale before
      // subscribing from its watermark; otherwise an older query hides a write.
      void queries.invalidateQueries()
      if (client.origin === LAN_ORIGIN) pairedSubscribe(next.server_id)
      else unsubscribe = client.subscribe('account', cursor, apply, error => {
        if (stale() || current !== generation) return
        if (error instanceof ApiError && error.code === 'resync_required') void start()
        else failed(error)
      })
    } catch (error) {
      if (stale() || current !== generation) return
      failed(error as Error)
      if (!(error instanceof ApiError && [401, 403, 404].includes(error.status))) retryTimer = setTimeout(() => { void start() }, retryDelayMs(++retry))
    }
  }
  const foreground = () => { if (!document.hidden && !stale()) { refreshSummaries(); if (useAccountSyncStore.getState().status === 'error') void start() } }
  document.addEventListener('visibilitychange', foreground)
  void start()
  return () => {
    disposed = true
    ++generation
    unsubscribe?.()
    cleanupConnection?.()
    sse?.abort()
    clearTimeout(refreshTimer)
    clearTimeout(retryTimer)
    document.removeEventListener('visibilitychange', foreground)
    useAccountSyncStore.setState(useAccountSyncStore.getInitialState(), true)
    for (const id of remoteRuns) useConversationActivityStore.getState().finishRun(id, 'cancelled')
  }
}
