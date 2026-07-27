import { AlertTriangle, GitBranch, Link2Off, Route } from 'lucide-react'
import type { TFunction } from 'i18next'
import { useTranslation } from 'react-i18next'

import { normalizeLanguage } from '@/i18n'
import { formatNumber } from '@/lib/format'
import { cn } from '@/lib/utils'
import type {
  AgentDispatchStatus,
  AgentDispatchTrace,
  PublicTurnArtifact,
  SchedulerActionKind,
  SchedulerSelectionReason,
} from '@/lib/api-v2/types'

interface DispatchDagProps {
  dispatches: readonly AgentDispatchTrace[]
  className?: string
}

type EdgeIssue = 'orphan' | 'cycle'

interface DagNode {
  dispatch: AgentDispatchTrace
  children: DagNode[]
  issue?: EdgeIssue
}

const MAX_VISUAL_DEPTH = 3

function findCycleRoots(
  dispatchById: ReadonlyMap<string, AgentDispatchTrace>,
  order: ReadonlyMap<string, number>,
): Set<string> {
  const processed = new Set<string>()
  const roots = new Set<string>()

  for (const startId of dispatchById.keys()) {
    if (processed.has(startId)) continue
    const path: string[] = []
    const pathIndex = new Map<string, number>()
    let currentId: string | null = startId

    while (currentId && dispatchById.has(currentId) && !processed.has(currentId)) {
      const cycleStart = pathIndex.get(currentId)
      if (cycleStart !== undefined) {
        const cycle = path.slice(cycleStart)
        const root = cycle.reduce((earliest, id) =>
          (order.get(id) ?? Number.MAX_SAFE_INTEGER) <
          (order.get(earliest) ?? Number.MAX_SAFE_INTEGER)
            ? id
            : earliest,
        )
        roots.add(root)
        break
      }
      pathIndex.set(currentId, path.length)
      path.push(currentId)
      currentId = dispatchById.get(currentId)?.parent_dispatch_id ?? null
    }

    for (const id of path) processed.add(id)
  }

  return roots
}

function buildDispatchForest(dispatches: readonly AgentDispatchTrace[]): DagNode[] {
  const dispatchById = new Map<string, AgentDispatchTrace>()
  const order = new Map<string, number>()
  for (const dispatch of dispatches) {
    if (!dispatchById.has(dispatch.id)) {
      order.set(dispatch.id, order.size)
      dispatchById.set(dispatch.id, dispatch)
    }
  }
  const cycleRoots = findCycleRoots(dispatchById, order)

  const nodes = new Map<string, DagNode>()
  for (const dispatch of dispatchById.values()) {
    const parentId = dispatch.parent_dispatch_id
    const issue = parentId && !dispatchById.has(parentId)
      ? 'orphan'
      : cycleRoots.has(dispatch.id)
        ? 'cycle'
        : undefined
    nodes.set(dispatch.id, { dispatch, children: [], issue })
  }

  const roots: DagNode[] = []
  for (const node of nodes.values()) {
    const parentId = node.dispatch.parent_dispatch_id
    const parent = parentId ? nodes.get(parentId) : undefined
    if (!parent || node.issue) {
      roots.push(node)
    } else {
      parent.children.push(node)
    }
  }
  return roots
}

interface FlatDagNode {
  node: DagNode
  depth: number
}

function flattenForest(forest: readonly DagNode[]): FlatDagNode[] {
  const flattened: FlatDagNode[] = []
  const stack = forest
    .map((node) => ({ node, depth: 0 }))
    .reverse()

  while (stack.length > 0) {
    const current = stack.pop()!
    flattened.push(current)
    for (let index = current.node.children.length - 1; index >= 0; index -= 1) {
      stack.push({ node: current.node.children[index], depth: current.depth + 1 })
    }
  }

  return flattened
}

const actionKeys = {
  speak: 'trace.actions.speak',
  call: 'trace.actions.call',
  handoff: 'trace.actions.handoff',
  wait: 'trace.actions.wait',
  silent: 'trace.actions.silent',
} as const satisfies Record<SchedulerActionKind, string>

const reasonKeys = {
  user_mention: 'trace.reasons.user_mention',
  agent_call: 'trace.reasons.agent_call',
  agent_handoff: 'trace.reasons.agent_handoff',
  agent_text_mention: 'trace.reasons.agent_text_mention',
  deterministic_order: 'trace.reasons.deterministic_order',
  moderator: 'trace.reasons.moderator',
  moderator_fallback: 'trace.reasons.moderator_fallback',
} as const satisfies Record<SchedulerSelectionReason, string>

const statusKeys = {
  queued: 'trace.dispatchStatuses.queued',
  running: 'trace.dispatchStatuses.running',
  completed: 'trace.dispatchStatuses.completed',
  silent: 'trace.dispatchStatuses.silent',
  waiting_for_user: 'trace.dispatchStatuses.waiting_for_user',
  interrupted: 'trace.dispatchStatuses.interrupted',
  cancelled: 'trace.dispatchStatuses.cancelled',
  failed: 'trace.dispatchStatuses.failed',
} as const satisfies Record<AgentDispatchStatus, string>

function hasOwnKey<T extends object>(record: T, key: PropertyKey): key is keyof T {
  return Object.prototype.hasOwnProperty.call(record, key)
}

function actionLabel(value: string, t: TFunction<'chat'>) {
  return hasOwnKey(actionKeys, value)
    ? t(actionKeys[value])
    : t('common:wireLabels.unknownAction', { value })
}

function statusLabel(value: string, t: TFunction<'chat'>) {
  return hasOwnKey(statusKeys, value)
    ? t(statusKeys[value])
    : t('common:wireLabels.unknownDispatchStatus', { value })
}

function reasonLabel(value: string, t: TFunction<'chat'>) {
  return hasOwnKey(reasonKeys, value)
    ? t(reasonKeys[value])
    : t('common:wireLabels.unknownSelectionReason', { value })
}

function ArtifactDetails({ artifact }: { artifact: PublicTurnArtifact | null }) {
  const { t } = useTranslation('chat')
  if (!artifact) return null
  return (
    <dl className="mt-2 grid grid-cols-[auto_minmax(0,1fr)] gap-x-2 gap-y-1 border-t border-border pt-2 text-2xs">
      {artifact.mode ? <><dt className="text-muted-foreground">{t('trace.mode')}</dt><dd>{hasOwnKey(actionKeys, artifact.mode) ? t(actionKeys[artifact.mode]) : artifact.mode}</dd></> : null}
      {artifact.target_agent_id ? <><dt className="text-muted-foreground">{t('trace.target')}</dt><dd className="truncate" title={artifact.target_agent_id}>{artifact.target_agent_id}</dd></> : null}
      {artifact.child_dispatch_id ? <><dt className="text-muted-foreground">{t('trace.child')}</dt><dd className="truncate" title={artifact.child_dispatch_id}>{artifact.child_dispatch_id}</dd></> : null}
      {artifact.outcome ? <><dt className="text-muted-foreground">{t('trace.outcome')}</dt><dd className="break-words">{artifact.outcome}</dd></> : null}
      {artifact.failure_code ? <><dt className="text-muted-foreground">{t('trace.failure')}</dt><dd className="break-words text-destructive">{artifact.failure_code}</dd></> : null}
    </dl>
  )
}

function DagRow({ node, depth }: FlatDagNode) {
  const { t, i18n } = useTranslation('chat')
  const language = normalizeLanguage(i18n.resolvedLanguage ?? i18n.language) ?? 'en-US'
  const IssueIcon = node.issue === 'orphan' ? Link2Off : AlertTriangle
  const dispatch = node.dispatch
  return (
    <li
      className="relative min-w-0"
      data-dispatch-id={dispatch.id}
      data-visual-depth={Math.min(depth, MAX_VISUAL_DEPTH)}
      style={{ paddingInlineStart: `${Math.min(depth, MAX_VISUAL_DEPTH) * 12}px` }}
    >
      <div className="min-w-0 rounded-md border border-border bg-background px-3 py-2">
        <div className="flex min-w-0 flex-wrap items-center gap-2">
          <Route className="h-3.5 w-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
          <span className="min-w-0 truncate font-mono text-xs font-semibold" title={dispatch.target_agent_id}>
            {dispatch.target_agent_id}
          </span>
          <span className="rounded-[3px] bg-muted px-1.5 py-0.5 text-[10px] capitalize text-muted-foreground">
            {actionLabel(dispatch.action_kind, t)}
          </span>
          <span className={cn('ml-auto text-[10px] capitalize text-muted-foreground', dispatch.status === 'failed' && 'text-destructive')}>
            {statusLabel(dispatch.status, t)}
          </span>
        </div>
        <div className="mt-1 flex flex-wrap gap-x-3 gap-y-1 text-2xs text-muted-foreground">
          <span>{reasonLabel(dispatch.selection_reason, t)}</span>
          <span>{t('trace.hop', { count: formatNumber(dispatch.hop, language) })}</span>
          <span>{t('trace.tokenCount', { count: formatNumber(dispatch.total_tokens, language) })}</span>
          {node.issue ? (
            <span className="inline-flex items-center gap-1 text-warning-foreground" data-edge-issue={node.issue}>
              <IssueIcon className="h-3 w-3" aria-hidden="true" />
              {node.issue === 'orphan' ? t('trace.missingParent') : t('trace.cycleDetached')}
            </span>
          ) : null}
        </div>
        {dispatch.failure_code ? <p className="mt-1 break-words text-2xs text-destructive">{t('trace.failureDetail', { message: dispatch.failure_code })}</p> : null}
        <ArtifactDetails artifact={dispatch.artifact} />
      </div>
    </li>
  )
}

export function DispatchDag({ dispatches, className }: DispatchDagProps) {
  const { t } = useTranslation('chat')
  const forest = buildDispatchForest(dispatches)
  const flattened = flattenForest(forest)
  if (forest.length === 0) {
    return <p className={cn('py-6 text-center text-sm text-muted-foreground', className)}>{t('trace.noDispatches')}</p>
  }
  return (
    <div className={cn('min-w-0', className)}>
      <div className="mb-2 flex items-center gap-2 text-xs font-medium text-muted-foreground">
        <GitBranch className="h-3.5 w-3.5" aria-hidden="true" />
        {t('trace.dispatchPath')}
      </div>
      <ul className="space-y-2" aria-label={t('trace.dispatchPath')}>
        {flattened.map(({ node, depth }) => (
          <DagRow key={node.dispatch.id} node={node} depth={depth} />
        ))}
      </ul>
    </div>
  )
}
