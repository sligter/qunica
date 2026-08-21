/**
 * Parsing the app-control tool results the Assistant emits.
 *
 * `AppPropose` and `AppPrefill` return JSON in the tool result's `output`.
 * Anything that fails to parse is simply not a pending action: the timeline
 * still renders the tool row, it just gets no card.
 */

/** A change staged for approval. */
export interface StagedAppAction {
  action_id: string
  target_kind: string
  action: string
  summary: string
}

/** A change the Assistant cannot make, handed back as a form to open. */
export interface PrefillAppAction {
  route: string
  fields: Record<string, unknown>
}

export type PendingAppAction = StagedAppAction | PrefillAppAction

export function isPrefill(action: PendingAppAction): action is PrefillAppAction {
  return 'route' in action
}

/**
 * Query keys to refresh after a staged change is applied, by target kind.
 *
 * Without this the user approves a change and the list they are looking at
 * keeps showing the old state until something else happens to refetch.
 */
export function queryKeysForKind(kind: string): string[][] {
  switch (kind) {
    case 'agent':
      return [['agents']]
    case 'provider':
      return [['llm-providers']]
    case 'mcp':
      return [['mcp-servers']]
    case 'skill':
      return [['skills']]
    case 'workspace':
      return [['workspaces']]
    case 'group':
      return [['groups']]
    case 'group_template':
      return [['group-templates']]
    case 'group_note':
      return [['groups']]
    case 'chat':
      return [['direct-chats']]
    default:
      return []
  }
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null
}

function asString(value: unknown): string | null {
  return typeof value === 'string' && value.trim() ? value : null
}

/**
 * Read a pending action out of a tool result's raw `output`.
 *
 * Returns null for every other tool, so the caller can run this over any
 * result without knowing which tool produced it.
 */
export function pendingActionFromOutput(output: string | undefined): PendingAppAction | null {
  if (!output) return null
  let parsed: unknown
  try {
    parsed = JSON.parse(output)
  } catch {
    return null
  }
  const record = asRecord(parsed)
  if (!record) return null

  const actionId = asString(record.action_id)
  if (actionId) {
    return {
      action_id: actionId,
      target_kind: asString(record.target_kind) ?? 'unknown',
      action: asString(record.action) ?? 'update',
      summary: asString(record.summary) ?? asString(record.message) ?? actionId,
    }
  }

  const route = asString(record.route)
  if (route) {
    return { route, fields: asRecord(record.fields) ?? {} }
  }

  return null
}
