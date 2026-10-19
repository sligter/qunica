import { fetchJson } from '@/lib/api-v2/client'
import { apiOrigin } from '@/lib/runtime'
import { LAN_ORIGIN } from '@/lib/androidSession'
import { realtimeClient } from './client'

export async function cancelTurn(conversationId: string, turnId: string, token: string): Promise<unknown> {
  if (apiOrigin() === LAN_ORIGIN) return fetchJson(`/groups/${conversationId}/turns/${turnId}/cancel`, { method: 'POST', token })
  await realtimeClient(token).command({ request_id: crypto.randomUUID(), name: 'turn.cancel', target: { turn_id: turnId }, payload: {} })
  return fetchJson(`/groups/${conversationId}/turns/${turnId}`, { token })
}
