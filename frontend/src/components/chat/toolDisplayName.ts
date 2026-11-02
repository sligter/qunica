/**
 * The name a tool call is shown under, in one place for both the live
 * timeline and the persisted turn details.
 *
 * A reloaded turn renders through `PersistedTurnDetails` while a running one
 * renders through `StreamTimeline`; keeping two label maps would let the same
 * call read "DispatchTask" before a reload and "派发任务" after it.
 *
 * `DispatchTask` is the only call whose name depends on what it did: passing a
 * `task_id` continues an existing task, omitting it starts a new one. That is
 * read from the recorded arguments rather than matched inside the rendered
 * summaries, because a summary is cut at a fixed length — a continuation whose
 * instructions are long would lose the marker and silently show the wrong
 * label.
 */

/** Just enough of i18next's `t` for key lookups performed here. */
export type Translate = (key: string) => string

const DISPATCH_TASK = 'DispatchTask'

function parseRecord(raw: string | null | undefined): Record<string, unknown> | null {
  if (!raw) return null
  try {
    const value: unknown = JSON.parse(raw)
    return typeof value === 'object' && value !== null ? (value as Record<string, unknown>) : null
  } catch {
    // Truncated or non-JSON summaries fall back to the default label.
    return null
  }
}

/** Whether a DispatchTask call continued a task instead of starting one. */
export function continuedTask(argsSummary: string | null | undefined, resultSummary: string | null | undefined): boolean {
  const args = parseRecord(argsSummary)
  if (typeof args?.task_id === 'string' && args.task_id.trim() !== '') return true
  return parseRecord(resultSummary)?.status === 'continued'
}

export function toolDisplayName(
  toolName: string | null | undefined,
  t: Translate,
  summary: { argsSummary?: string | null; resultSummary?: string | null } = {},
): string {
  if (!toolName || toolName === 'Unknown tool') return t('messages.unknownTool')
  if (toolName === DISPATCH_TASK) {
    return t(continuedTask(summary.argsSummary, summary.resultSummary) ? 'tools.continueTask' : 'tools.dispatchTask')
  }
  return toolName
}